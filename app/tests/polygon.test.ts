import assert from 'node:assert/strict';
import { test } from 'node:test';
import { isValidRing, nearestOnRing, reshape, signedArea } from '../src/editor/polyedit.ts';
import { reshapeTool } from '../src/editor/tools/polygonGesture.ts';
import type { Command, Point, ZoneItem } from '../src/core/types.ts';
import type { ToolEnv, PointerInfo } from '../src/editor/tools/types.ts';

const p = (x: number, y: number): Point => ({ x: x*1e6, y: y*1e6 });
const ring = [p(0,0),p(10,0),p(10,10),p(0,10)];
const zone = (layer = 0): ZoneItem => ({ kind:'zone', outline:ring, fill:[], net:'GND', name:'Ground', layer,
  priority:2, clearance:2e5, minWidth:0, connect:'thermal', thermalGap:3e5, spokeWidth:3e5, keepout:false, removeIslands:false });
const event: PointerInfo = { sx:0, sy:0, shift:false, ctrl:false, alt:false };

function harness(mode: 'cut'|'slice'|'reshape', response: () => Promise<boolean> = async () => true, grid = 0) {
  let cursor = p(0,0);
  const commands: Command[] = [];
  const doc = { items: new Map([[1, zone()]]), selection:new Set<number>(), activeLayer:0, singleLayer:false };
  const env = {
    doc:() => doc, raw:() => cursor,
    cursor:() => grid ? {x:Math.round(cursor.x/grid)*grid,y:Math.round(cursor.y/grid)*grid} : cursor, scale:() => 100/1e6,
    select:(ids: Iterable<number>) => { doc.selection = new Set(ids); }, redraw:() => {},
    exec: async (cmd: Command) => {
      commands.push(cmd);
      const ok = await response();
      if (ok && cmd.op === 'editPolygon') {
        const source = doc.items.get(cmd.id)!;
        doc.items.set(cmd.id, {...source, outline:cmd.outlines[0]});
        for (const outline of cmd.outlines.slice(1)) doc.items.set(Math.max(...doc.items.keys())+1,{...source,outline});
      }
      return ok;
    },
  } as unknown as ToolEnv;
  const tool = reshapeTool(mode);
  const move = (x:number,y:number,shift=false) => { cursor=p(x,y); tool.move?.(env,{...event,shift}); };
  const click = (x:number,y:number,shift=false) => { move(x,y,shift); tool.down?.(env,{...event,shift}); };
  const key = (key:string) => tool.key?.(env,key,{shiftKey:false} as KeyboardEvent);
  const tick = async () => { await new Promise(resolve => setImmediate(resolve)); };
  return { doc, commands, tool, env, click, move, key, tick };
}

test('cut previews, removes the clicked side, commits once and stays ready', async () => {
  const h=harness('cut');
  h.click(5,0); h.click(5,10);
  assert.equal(h.commands.length,0,'finishing the path must wait for a side choice');
  assert.match(h.tool.hint(h.env),/RED side/);
  assert.equal(h.tool.overlayFills?.(h.env).length,2);
  h.click(8,5); await h.tick();
  assert.equal(h.commands.length,1);
  const cmd=h.commands[0]; assert.equal(cmd.op,'editPolygon');
  if(cmd.op!=='editPolygon') throw Error();
  assert.equal(cmd.id,1); assert.equal(cmd.outlines.length,1);
  assert.ok(cmd.outlines[0].every(q=>q.x<=5e6));
  assert.match(h.tool.hint(h.env),/click a polygon boundary/);
});

test('slice commits two valid halves immediately; rapid second clicks cannot commit twice', async () => {
  let resolve!: (v:boolean)=>void;
  const h=harness('slice',()=>new Promise(r=>{resolve=r;}));
  h.click(5,0); h.click(5,10); h.click(5,10); h.key('Enter');
  assert.equal(h.commands.length,1);
  const cmd=h.commands[0]; if(cmd.op!=='editPolygon') throw Error();
  assert.equal(cmd.outlines.length,2);
  assert.deepEqual(cmd.outlines.map(r=>Math.abs(signedArea(r))),[50e12,50e12]);
  assert.ok(cmd.outlines.every(isValidRing));
  resolve(true); await h.tick(); assert.match(h.tool.hint(h.env),/click a polygon boundary/);
});

test('corners, Backspace, cancel and outside side clicks do not mutate', () => {
  const h=harness('cut');
  h.click(5,0); h.click(6,4); h.key('Backspace'); h.click(5,10);
  h.click(15,5); assert.equal(h.commands.length,0);
  assert.match(h.tool.hint(h.env),/Click inside/);
  h.key('Backspace'); assert.match(h.tool.hint(h.env),/click corners/);
  assert.equal(h.tool.cancel(h.env),true); assert.equal(h.commands.length,0);
  assert.equal(h.tool.cancel(h.env),false);
});

test('cut and slice reject paths that leave the original boundary', () => {
  for(const mode of ['cut','slice'] as const) {
    const h=harness(mode); h.click(0,5); h.click(-5,5); h.click(-5,15); h.click(5,15); h.click(5,10);
    assert.equal(h.commands.length,0); assert.match(h.tool.hint(h.env),/must stay inside/);
    assert.equal(h.tool.overlayFills?.(h.env).length,0);
  }
});

test('reshape can extend outside, while preserving both preview choices', async () => {
  const h=harness('reshape'); h.click(0,5); h.click(-5,5); h.click(-5,15); h.click(5,15); h.click(5,10);
  await h.tick(); assert.equal(h.commands.length,1);
  const cmd=h.commands[0]; if(cmd.op!=='editPolygon') throw Error();
  assert.ok(isValidRing(cmd.outlines[0]));
  assert.ok(Math.abs(signedArea(cmd.outlines[0]))>Math.abs(signedArea(ring)));
});

test('a failed command preserves the cut and side choice for retry', async () => {
  const h=harness('cut',async()=>false); h.click(5,0); h.click(5,10); h.click(8,5); await h.tick();
  assert.equal(h.commands.length,1); assert.match(h.tool.hint(h.env),/Could not apply/);
  assert.equal(h.tool.overlayFills?.(h.env).length,2);
});

test('hidden layers are ignored unless selected and not in single-layer mode', () => {
  const h=harness('slice'); h.doc.items.set(1,zone(33)); h.click(5,0); h.click(5,10);
  assert.equal(h.commands.length,0); assert.match(h.tool.hint(h.env),/click a polygon boundary/);
  h.doc.selection.add(1); h.doc.singleLayer=true; h.click(5,0);
  assert.match(h.tool.hint(h.env),/click a polygon boundary/);
  h.doc.singleLayer=false; h.click(5,0); assert.match(h.tool.hint(h.env),/click corners/);
});

test('geometry rejects repeated vertices, T junctions, retracing and crossings', () => {
  for(const r of [
    [p(0,0),p(10,0),p(10,10),p(0,0),p(0,10)],
    [p(0,0),p(10,0),p(10,10),p(5,0),p(0,10)],
    [p(0,0),p(10,0),p(5,0),p(5,10),p(0,10)],
    [p(0,0),p(10,10),p(0,10),p(10,0)],
  ]) assert.equal(isValidRing(r),false);
});

test('same-edge notch partitions correctly and winding/translation do not change area', () => {
  for(const original of [ring,[...ring].reverse(),ring.map(q=>({x:q.x+1e12,y:q.y-1e12}))]) {
    const offset=original===ring||original[0].x<1e11?p(0,0):{x:1e12,y:-1e12};
    const at=(x:number,y:number)=>({x:p(x,y).x+offset.x,y:p(x,y).y+offset.y});
    const a=nearestOnRing(original,at(2,0))!, b=nearestOnRing(original,at(8,0))!;
    const r=reshape(original,a,b,[a.point,at(2,3),at(8,3),b.point]);
    assert.equal(r.partition,true); assert.ok(r.valid.every(Boolean));
    assert.deepEqual(r.options.map(q=>Math.abs(signedArea(q))),[82e12,18e12]);
  }
});

test('Shift constrains corners and reset invalidates pending completion callbacks', async () => {
  let resolve!: (v:boolean)=>void;
  const h=harness('slice',()=>new Promise(r=>{resolve=r;}));
  h.click(5,0); h.click(7,3,true); h.move(7,3);
  const lines=h.tool.overlay?.(h.env)??[];
  assert.ok(lines.some(l=>l.a.x===5e6 && l.a.y===0 && l.b.x===8e6 && l.b.y===3e6));
  h.key('Backspace'); h.click(5,10); assert.equal(h.commands.length,1);
  h.tool.reset?.(); resolve(true); await h.tick();
  assert.match(h.tool.hint(h.env),/click a polygon boundary/);
});

test('a concave polygon slices without crossing its notch', () => {
  const concave=[p(0,0),p(10,0),p(10,5),p(5,5),p(5,10),p(0,10)];
  const a=nearestOnRing(concave,p(2,0))!, b=nearestOnRing(concave,p(2,10))!;
  const r=reshape(concave,a,b,[a.point,b.point]);
  assert.equal(r.partition,true);
  assert.deepEqual(r.options.map(q=>Math.abs(signedArea(q))),[55e12,20e12]);
  const c=nearestOnRing(concave,p(10,2))!, d=nearestOnRing(concave,p(2,10))!;
  assert.equal(reshape(concave,c,d,[c.point,p(8,8),d.point]).partition,false);
});

test('corners follow the free mouse position even when the board grid is coarse', async () => {
  const h=harness('slice',async()=>true,2e6);
  h.click(4.137,0); h.click(3.271849,4.183627); h.click(5.763,10);
  await h.tick();
  assert.equal(h.commands.length,1);
  const cmd=h.commands[0]; if(cmd.op!=='editPolygon') throw Error();
  assert.ok(cmd.outlines.some(r=>r.some(q=>q.x===3271849 && q.y===4183627)));
});

test('a second and third cut can start on the new diagonal boundary without reactivating', async () => {
  const h=harness('cut');
  h.click(2,0); h.click(8,10); h.click(9,5); await h.tick();
  assert.equal(h.commands.length,1);
  // The new diagonal x=2+0.6*y has non-integer projections in nanometers.
  h.click(4.123457,3.539091); h.click(0,7.213891);
  assert.match(h.tool.hint(h.env),/RED side/);
  h.click(1,9); await h.tick();
  assert.equal(h.commands.length,2,h.tool.hint(h.env));
  const second=h.commands[1]; if(second.op!=='editPolygon') throw Error();
  assert.deepEqual(second.original,(h.commands[0] as Extract<Command,{op:'editPolygon'}>).outlines[0]);
  const current=h.doc.items.get(1)!.outline;
  const hit=nearestOnRing(current,p(3.5,2.5))!;
  h.click(hit.point.x/1e6,hit.point.y/1e6); h.click(0,2.2); h.click(0.2,0.2); await h.tick();
  assert.equal(h.commands.length,3,h.tool.hint(h.env));
});

test('screenshot regression: trim a small corner between an existing bevel and vertical edge', async () => {
  const h=harness('cut');
  // Same corner as the screenshot: an existing shallow bevel then a right edge.
  const clipped=[p(0,0),p(20,0),p(20,16),p(13.5,20),p(0,20)];
  h.doc.items.set(1,{...zone(),outline:clipped});
  h.click(17.9317,17.2728); h.click(20,11.4761);
  assert.match(h.tool.hint(h.env),/RED side/);
  h.click(19.7,15); await h.tick();
  assert.equal(h.commands.length,1);
  const current=h.doc.items.get(1)!.outline;
  const at=nearestOnRing(current,p(18.5,15.5))!;
  h.click(at.point.x/1e6,at.point.y/1e6); h.click(20,8.3917);
  assert.match(h.tool.hint(h.env),/RED side/);
  h.click(19.8,11); await h.tick();
  assert.equal(h.commands.length,2);
});
