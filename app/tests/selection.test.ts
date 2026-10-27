import test from 'node:test';
import assert from 'node:assert/strict';
import { Scene } from '../src/editor/scene.ts';
import { DEFAULT_VIEW } from '../src/core/view.ts';
import type { Item, Point } from '../src/core/types.ts';
import type { DocState } from '../src/core/store.ts';
import type { ToolEnv, PointerInfo } from '../src/editor/tools/types.ts';

const prefs = new Map<string, string>();
Object.defineProperty(globalThis, 'localStorage', { configurable: true, value: {
  getItem: (key: string) => prefs.get(key) ?? null,
  setItem: (key: string, value: string) => prefs.set(key, value),
  removeItem: (key: string) => prefs.delete(key),
} });
const { useSelectFilter, selectable } = await import('../src/core/selectFilter.ts');
const { selectTool } = await import('../src/editor/tools/select.ts');
const { useEditor } = await import('../src/core/store.ts');
const { selectAll } = await import('../src/core/actions.ts');
const p = (x: number, y: number): Point => ({ x: x * 1e6, y: y * 1e6 });
const via = (x: number, y: number): Item => ({ kind: 'via', pos: p(x,y), diameter: 600000, drill: 300000, fromLayer: 0, toLayer: 1, net: 'GND' });
const track: Item = { kind: 'track', a: p(-2,4), b: p(10,4), width: 200000, layer: 0, net: 'GND' };

function harness() {
  useSelectFilter.getState().all();
  const items = new Map<number, Item>([[1, via(3,3)], [2, via(6,6)], [3, track]]);
  const scene = new Scene();
  scene.update(items, { kind: 'pcb', cache: {}, footprints: {}, view: DEFAULT_VIEW });
  const doc = { info: { id: 'test', kind: 'pcb' }, items, selection: new Set<number>(), marked: new Set<number>(), activeLayer: 0, tool: 'select', view: DEFAULT_VIEW, grid: 1e6, footprints: {} } as unknown as DocState;
  useEditor.setState({ docs: { test: doc }, activeDocId: 'test' });
  let cursor = p(0,0);
  const commands: import("../src/core/types.ts").Command[] = [];
  const env = {
    docId: 'test', doc: () => useEditor.getState().docs.test, scene,
    raw: () => cursor, cursor: () => cursor, scale: () => 10 / 1e6,
    select: (ids: Iterable<number>) => useEditor.getState().patchDoc('test', { selection: new Set(ids) }),
    toWorld: (sx: number, sy: number) => p(sx / 10, sy / 10),
    redraw: () => {}, setBox: () => {},
    ctx: () => ({kind:"pcb",cache:{},footprints:useEditor.getState().docs.test.footprints,view:DEFAULT_VIEW}),
    exec: async (cmd: import("../src/core/types.ts").Command) => { commands.push(cmd); return true; },
  } as unknown as ToolEnv;
  const tool = selectTool();
  const event = (x: number, y: number, ctrl = false, shift = false): PointerInfo => { cursor = p(x,y); return { sx: x*10, sy: y*10, ctrl, shift, alt: false }; };
  const click = (x: number, y: number, ctrl = false) => { tool.down!(env, event(x,y,ctrl)); tool.up!(env,event(x,y,ctrl)); };
  const box = (x1: number,y1: number,x2: number,y2: number,ctrl = false,shift = false) => {
    tool.down!(env,event(x1,y1,ctrl,shift)); tool.move!(env,event(x2,y2,ctrl,shift)); tool.up!(env,event(x2,y2,ctrl,shift));
  };
  const selected = () => [...env.doc().selection].sort();
  const move=(x:number,y:number)=>tool.move!(env,event(x,y));
  const key=(key:string,shiftKey=false)=>tool.key!(env,key,{shiftKey}as KeyboardEvent);
  return { env, scene, items, click, box, selected, move, key, tool, commands };
}

test('vias-only box selection ignores tracks and keeps both vias', () => {
  const h = harness(); useSelectFilter.getState().only('vias');
  h.box(0,0,8,8); assert.deepEqual(h.selected(), [1,2]);
  assert.equal(selectable('pcb', track), false);
  assert.equal(selectable('schematic', track), true);
});

test('Ctrl+click adds individual objects and toggles an existing member off', () => {
  const h = harness(); useSelectFilter.getState().only('vias');
  h.click(3,3); h.click(6,6,true); assert.deepEqual(h.selected(), [1,2]);
  h.click(3,3,true); assert.deepEqual(h.selected(), [2]);
});

test('Ctrl and Shift make a rectangular selection additive', () => {
  for (const modifier of ['ctrl','shift']) {
    const h = harness(); useSelectFilter.getState().only('vias'); h.click(3,3);
    h.box(5,5,7,7,modifier === 'ctrl',modifier === 'shift'); assert.deepEqual(h.selected(), [1,2]);
  }
});

test('left-to-right requires enclosure; right-to-left includes a crossing track', () => {
  const h = harness(); h.box(0,0,8,8); assert.deepEqual(h.selected(), [1,2]);
  h.box(8,0,0,8); assert.deepEqual(h.selected(), [1,2,3]);
});

test('None blocks click and box picks; All restores them', () => {
  const h = harness(); useSelectFilter.getState().none();
  h.click(3,3); h.box(8,0,0,8); assert.deepEqual(h.selected(), []);
  useSelectFilter.getState().all(); h.box(8,0,0,8); assert.deepEqual(h.selected(), [1,2,3]);
});

test('Ctrl+A honors the same selectable object types', () => {
  const h = harness(); useSelectFilter.getState().only('vias'); selectAll();
  assert.deepEqual(h.selected(), [1,2]);
});

function footprintHarness() {
 const h=harness();
 const pad=(x:number):Item=>({kind:'pad',number:String(x),pos:p(x,0),width:6e5,height:1e6,rotation:0,shape:'rect',padKind:'smd',layer:0,drill:null,cornerRatio:0});
 const footprint={name:'Test',description:'',items:[pad(-1),pad(1)],model:null};
 h.items.set(4,{kind:'footprint',pos:p(20,20),rotation:0,side:'top',locked:false,footprint:'F',designator:'U1',value:'',padNets:{},component:'',parameters:{}});
 const outline=[p(15,15),p(25,15),p(25,25),p(15,25)];
 h.items.set(5,{kind:'zone',outline,fill:[{outer:outline,holes:[]}],layer:0,net:'GND',name:'',priority:0,clearance:0,minWidth:0,connect:'thermal',thermalGap:0,spokeWidth:0,keepout:false,removeIslands:false});
 useEditor.getState().patchDoc('test',{footprints:{F:footprint}});
 h.scene.update(h.items,h.env.ctx());return h;
}

test('clicking between footprint pads selects its body above a filled pour',()=>{
 const h=footprintHarness();h.click(20,20);assert.deepEqual(h.selected(),[4]);
});

test('M picks up a footprint under the cursor and click commits its move',()=>{
 const h=footprintHarness();h.move(20,20);assert.equal(h.key('m'),true);
 h.move(23,22);h.click(23,22);
 assert.deepEqual(h.commands,[{op:'move',ids:[4],dx:3e6,dy:2e6}]);
});

test('cancelling a floating M move leaves geometry and history untouched',()=>{
 const h=footprintHarness();h.move(20,20);h.key('m');h.move(24,21);
 assert.equal(h.tool.cancel(h.env),true);assert.deepEqual(h.commands,[]);assert.deepEqual(h.tool.preview!(h.env),[]);
});

test('R rotates a hovered footprint about its origin instead of the pointer',()=>{
 const h=footprintHarness();h.move(20.2,20.2);assert.equal(h.key('r'),true);
 assert.deepEqual(h.commands,[{op:'rotate',ids:[4],center:p(20,20),angle:90}]);
});

test('Esc cancels an M move and its temporary R rotation together',()=>{
 const h=footprintHarness();h.move(20,20);h.key('m');h.key('r');h.move(23,22);
 const preview=h.tool.preview!(h.env)[0];assert.equal(preview.kind,'footprint');if(preview.kind==='footprint')assert.equal(preview.rotation,90);
 h.tool.cancel(h.env);assert.deepEqual(h.commands,[]);assert.equal(h.env.doc().items.get(4)!.kind,'footprint');
});
test('M move plus R rotation commits one undoable replacement',()=>{
 const h=footprintHarness();h.move(20,20);h.key('m');h.key('r');h.move(23,22);h.click(23,22);
 assert.equal(h.commands.length,1);const command=h.commands[0];assert.equal(command.op,'replaceMany');
 if(command.op==='replaceMany'){const fp=command.items[0].item;assert.equal(fp.kind,'footprint');if(fp.kind==='footprint'){assert.equal(fp.rotation,90);assert.deepEqual(fp.pos,p(23,22));}}
});
test('Ctrl toggles a box selection and Ctrl+Shift subtracts from it',()=>{
 const h=harness();useSelectFilter.getState().only('vias');h.click(3,3);h.click(6,6,true);h.box(0,0,5,5,true);assert.deepEqual(h.selected(),[2]);h.box(5,5,7,7,true,true);assert.deepEqual(h.selected(),[]);
});
