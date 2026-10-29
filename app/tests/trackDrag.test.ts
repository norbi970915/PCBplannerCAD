import test from 'node:test';
import assert from 'node:assert/strict';
import { applyTrackDrag, planTrackDrag, trackDragIds } from '../src/editor/trackDrag.ts';
import type { Item } from '../src/core/types.ts';

const mm = (v: number) => Math.round(v * 1e6);
const p = (x: number, y: number) => ({ x: mm(x), y: mm(y) });
const tr = (a: { x: number; y: number }, b: { x: number; y: number }, net = 'N'): Item => ({ kind: 'track', a, b, width: mm(0.25), layer: 0, net });

// 45° in, horizontal middle, 45° out: (0,0)→(2,2)→(6,2)→(8,0).
const board = () => new Map<number, Item>([[1, tr(p(0, 0), p(2, 2))], [2, tr(p(2, 2), p(6, 2))], [3, tr(p(6, 2), p(8, 0))]]);

test('dragging a middle segment keeps the 45° neighbours connected and at 45°', () => {
  const items = board(); const plan = planTrackDrag(items, {}, [2])!;
  assert.deepEqual(trackDragIds(plan).sort(), [1, 2, 3]);
  const { replace, add } = applyTrackDrag(plan, p(0.3, 1));
  const out = new Map(replace.map((r) => [r.id, r.item]));
  assert.deepEqual(add, []);
  assert.deepEqual(out.get(2)!.a, p(3, 3)); assert.deepEqual(out.get(2)!.b, p(5, 3), 'moved straight up, ends slid along the diagonals');
  assert.deepEqual(out.get(1)!.b, p(3, 3)); assert.deepEqual(out.get(1)!.a, p(0, 0), 'far ends stay put');
  assert.deepEqual(out.get(3)!.a, p(5, 3)); assert.deepEqual(out.get(3)!.b, p(8, 0));
});

test('sliding a single segment along itself changes nothing', () => {
  const plan = planTrackDrag(board(), {}, [2])!;
  const out = new Map(applyTrackDrag(plan, p(1.5, 0)).replace.map((r) => [r.id, r.item]));
  assert.deepEqual(out.get(2)!.a, p(2, 2)); assert.deepEqual(out.get(2)!.b, p(6, 2));
});

test('an end held by a via gets a joining segment instead of leaving a gap', () => {
  const items = board(); items.delete(3);
  items.set(9, { kind: 'via', pos: p(6, 2), diameter: mm(0.6), drill: mm(0.3), fromLayer: 0, toLayer: 1, net: 'N' });
  const { replace, add } = applyTrackDrag(planTrackDrag(items, {}, [2])!, p(0, 1));
  const moved = replace.find((r) => r.id === 2)!.item;
  assert.deepEqual(moved.b, p(6, 3));
  assert.deepEqual(add.map((t) => [t.a, t.b]), [[p(6, 2), p(6, 3)]], 'the via stays and a stub reconnects it');
});

test('other nets and other layers are never dragged along', () => {
  const items = board(); items.set(4, tr(p(6, 2), p(6, 9), 'OTHER')); items.set(5, { ...tr(p(6, 2), p(9, 9)), layer: 1 } as Item);
  assert(!trackDragIds(planTrackDrag(items, {}, [2])!).some((id) => id === 4 || id === 5));
});

test('selections with non-track items fall back to an ordinary move', () => {
  const items = board(); items.set(7, { kind: 'via', pos: p(1, 1), diameter: 1, drill: 1, fromLayer: 0, toLayer: 1, net: 'N' });
  assert.equal(planTrackDrag(items, {}, [2, 7]), null);
});

test('a T-junction stretches every connected branch to the new point', () => {
  const items = board(); items.set(4, tr(p(6, 2), p(6, 6)));
  const out = new Map(applyTrackDrag(planTrackDrag(items, {}, [2])!, p(0, 1)).replace.map((r) => [r.id, r.item]));
  assert.deepEqual(out.get(2)!.b, p(6, 3)); assert.deepEqual(out.get(3)!.a, p(6, 3)); assert.deepEqual(out.get(4)!.a, p(6, 3));
});

test('moving a part previews its pad segment sliding at the same angle along its neighbour', async () => {
  const { attachedTracks, stretchAttached, attachedIds } = await import('../src/editor/trackDrag.ts');
  const pad = { kind: 'pad', number: '1', pos: p(0, 0), width: mm(1), height: mm(1), rotation: 0, shape: 'rect', padKind: 'smd', layer: 0, drill: null, cornerRatio: 0 };
  const items = new Map<number, Item>([
    [1, { kind: 'footprint', pos: p(20, 0), rotation: 0, side: 'top', footprint: 'F', padNets: { '1': 'N' }, designator: 'U2', value: '', component: '', locked: false, parameters: {} } as Item],
    [2, tr(p(15, 5), p(20, 0))], [3, tr(p(5, 5), p(15, 5))],
  ]);
  const att = attachedTracks(items, { F: { name: 'F', description: '', items: [pad], model: null } } as never, [1]);
  assert.deepEqual(attachedIds(att).sort(), [2, 3]);
  const [seg, nb] = stretchAttached(att, p(0, -3), true);
  assert.deepEqual([seg.a, seg.b], [p(12, 5), p(20, -3)], '45° kept, far end slid to (12,5)');
  assert.deepEqual([nb.a, nb.b], [p(5, 5), p(12, 5)], 'the horizontal neighbour got shorter');
  const [plain] = stretchAttached(att, p(0, -3), false);
  assert.deepEqual([plain.a, plain.b], [p(15, 5), p(20, -3)], 'Free mode stretches straight');
});

test('via previews keep both layers attached and split a trace passing through the via',async()=>{
 const {attachedTracks,stretchAttached}=await import('../src/editor/trackDrag.ts');
 const v:Item={kind:'via',pos:p(5,5),diameter:mm(.6),drill:mm(.3),fromLayer:0,toLayer:1,net:'N'};
 const items=new Map<number,Item>([[1,v],[2,tr(p(0,0),p(5,5))],[3,{...tr(p(5,5),p(9,9)),layer:1}as Item],[4,tr(p(5,5),p(9,5),'OTHER')]]);
 const attached=attachedTracks(items,{},[1]);assert.deepEqual(attached.map(a=>a.id),[2,3]);
 const preview=stretchAttached(attached,p(2,1));assert.deepEqual(preview[0].b,p(7,6));assert.deepEqual(preview[1].a,p(7,6));
 const inside=new Map<number,Item>([[1,v],[2,tr(p(0,0),p(10,10))]]);
 const split=stretchAttached(attachedTracks(inside,{},[1]),p(0,2));assert.equal(split.length,2);
 assert.deepEqual([split[0].a,split[0].b,split[1].a,split[1].b],[p(0,0),p(5,7),p(5,7),p(10,10)]);
});

test('arc attachment preview matches only the via net and physical layer span',async()=>{
 const {attachedViaArcs}=await import('../src/editor/trackDrag.ts');
 const arc={kind:'arcTrack',center:p(5,0),radius:mm(5),startAngle:90,sweep:90,width:mm(.2),layer:0,net:'N'}as Item;
 const items=new Map<number,Item>([[1,{kind:'via',pos:p(5,5),diameter:mm(.6),drill:mm(.3),fromLayer:0,toLayer:1,net:'N'}],[2,arc],[3,{...arc,net:'OTHER'}as Item]]);
 const attached=attachedViaArcs(items,[1]);assert.equal(attached.length,1);assert.deepEqual(attached[0].contacts,[{at:p(5,5),anchor:p(5,5)}]);
});

test('curved-track preview follows interior via contacts across wrapped and clockwise sweeps',async()=>{
 const {attachedViaArcs}=await import('../src/editor/trackDrag.ts');
 for(const[startAngle,sweep]of [[0,180],[330,180],[150,-180]]){
  const arc:Item={kind:'arcTrack',center:p(15,10),radius:mm(5),startAngle,sweep,width:mm(.2),layer:0,net:'N'};
  const items=new Map<number,Item>([[1,{kind:'via',pos:p(15,15.2),diameter:mm(.6),drill:mm(.3),fromLayer:0,toLayer:1,net:'N'}],[2,arc]]);
  const attached=attachedViaArcs(items,[1]);assert.equal(attached.length,1);assert.equal(attached[0].whole,false);
  assert.deepEqual(attached[0].contacts,[{at:p(15,15),anchor:p(15,15.2)}]);
 }
 const arc:Item={kind:'arcTrack',center:p(10,10),radius:mm(5),startAngle:330,sweep:60,width:mm(.2),layer:0,net:'N'};
 const opposite=new Map<number,Item>([[1,{kind:'via',pos:p(5,10),diameter:mm(.6),drill:mm(.3),fromLayer:0,toLayer:1,net:'N'}],[2,arc]]);
 assert.deepEqual(attachedViaArcs(opposite,[1]),[],'the missing part of a circle cannot attach');
});

test('a locked neighboring track is excluded from angle-preserving preview adjustment',async()=>{
 const {attachedTracks,stretchAttached}=await import('../src/editor/trackDrag.ts');
 const items=new Map<number,Item>([[1,{kind:'via',pos:p(8,0),diameter:mm(.6),drill:mm(.3),fromLayer:0,toLayer:1,net:'N'}],[2,tr(p(6,2),p(8,0))],[3,{...tr(p(2,2),p(6,2)),locked:true}as Item]]);
 const attached=attachedTracks(items,{},[1]);assert.equal(attached.length,1);assert.equal(attached[0].neighbour,undefined);
 const preview=stretchAttached(attached,p(0,1),true);assert.equal(preview.length,1);assert.deepEqual(preview[0].a,p(6,2));
});


test('multi-via preview splits an interior contact even when another selected via holds an endpoint',async()=>{
 const {attachedTracks,stretchAttached}=await import('../src/editor/trackDrag.ts');
 const via=(x:number):Item=>({kind:'via',pos:p(x,0),diameter:mm(.6),drill:mm(.3),fromLayer:0,toLayer:1,net:'N'});
 const items=new Map<number,Item>([[1,via(0)],[2,via(5)],[3,via(10)],[4,tr(p(0,0),p(10,0))]]);
 const attached=attachedTracks(items,{},[1,2]);assert.equal(attached.length,2);
 const preview=stretchAttached(attached,p(0,2));assert.deepEqual(preview.map(t=>[t.a,t.b]),[[p(0,2),p(5,2)],[p(5,2),p(10,0)]]);
});

test('dragging a track tethers a locked neighbor without hiding or changing it',()=>{
 const items=board();items.set(3,{...items.get(3)!,locked:true}as Item);
 const plan=planTrackDrag(items,{},[2])!;assert(!trackDragIds(plan).includes(3),'locked neighbor stays visible');
 const result=applyTrackDrag(plan,p(0,1));assert(!result.replace.some(r=>r.id===3),'locked neighbor stays exact');
 assert(result.add.some(t=>JSON.stringify(t.a)===JSON.stringify(p(6,2))&&JSON.stringify(t.b)===JSON.stringify(p(6,3))),'old junction is joined to the new segment');
});

 test('direction rails hold horizontal and diagonal runs through pointer noise, then release',async()=>{
 const {snapRouteDirection}=await import('../src/editor/routeDirection.ts');
 const a=snapRouteDirection(p(0,0),p(5,.03),mm(.05));assert.equal(a.axis,0);assert.deepEqual(a.point,p(5,0));
 const held=snapRouteDirection(p(0,0),p(5,.07),mm(.05),a.axis);assert.equal(held.axis,0);
 const released=snapRouteDirection(p(0,0),p(5,.2),mm(.05),held.axis);assert.equal(released.axis,null);assert.deepEqual(released.point,p(5,.2));
 const diagonal=snapRouteDirection(p(0,0),p(5,5.03),mm(.05));assert.equal(diagonal.axis,1);assert.equal(diagonal.point.x,diagonal.point.y);
 const reverse=snapRouteDirection(p(0,0),p(-5,0),mm(.05),0);assert.equal(reverse.axis,4);assert.deepEqual(reverse.point,p(-5,0));
 });

test('track slides ignore foreign-net vias and vias outside the dragged copper span',()=>{
 for(const foreign of [true,false]){
  const items=board();items.delete(3);
  items.set(0,{kind:'board',stackup:{layers:[0,33,34,1].map(layer=>({kind:'copper',layer}))}}as Item);
  items.set(9,{kind:'via',pos:p(6,2),diameter:mm(.6),drill:mm(.2),fromLayer:foreign?0:33,toLayer:foreign?1:34,net:foreign?'OTHER':'N'});
  const result=applyTrackDrag(planTrackDrag(items,{},[2])!,p(0,1));assert.equal(result.add.length,0,'unrelated copper must not create a fixed joining stub');
 }
});
