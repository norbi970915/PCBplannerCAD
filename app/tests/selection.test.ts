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

function harness(kind: DocState['info']['kind'] = 'pcb') {
  useSelectFilter.getState().reset();
  const items = new Map<number, Item>([[1, via(3,3)], [2, via(6,6)], [3, track]]);
  const scene = new Scene();
  scene.update(items, { kind, cache: {}, footprints: {}, view: DEFAULT_VIEW });
  const doc = { info: { id: 'test', kind }, items, selection: new Set<number>(), marked: new Set<number>(), activeLayer: 0, tool: 'select', view: DEFAULT_VIEW, grid: 1e6, footprints: {} } as unknown as DocState;
  useEditor.setState({ docs: { test: doc }, activeDocId: 'test' });
  let cursor = p(0,0);
  const commands: import("../src/core/types.ts").Command[] = [];
  const env = {
    docId: 'test', doc: () => useEditor.getState().docs.test, scene,
    raw: () => cursor, cursor: () => cursor, scale: () => 10 / 1e6,
    select: (ids: Iterable<number>) => useEditor.getState().patchDoc('test', { selection: new Set(ids) }),
    toWorld: (sx: number, sy: number) => p(sx / 10, sy / 10),
    redraw: () => {}, setBox: () => {},
    ctx: () => ({kind,cache:{},footprints:useEditor.getState().docs.test.footprints,view:DEFAULT_VIEW}),
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

test('schematic selection advertises its actual rotate and mirror controls', () => {
  const h = harness('schematic');
  h.env.select([1]);
  const hint = h.tool.hint!(h.env);
  assert.match(hint, /Drag selection to move/);
  assert.match(hint, /Space rotates/);
  assert.match(hint, /X mirrors/);
  assert.doesNotMatch(hint, /M move|R rotate|X route/);
  assert.equal(h.key('m'), false);
  assert.equal(h.key('r'), false);
  assert.equal(h.key(' '), true);
  assert.equal(h.commands.at(-1)?.op, 'rotate');
  assert.equal(h.key('x'), true);
  assert.equal(h.commands.at(-1)?.op, 'mirror');
});

test('footprint selection describes X as mirror rather than routing', () => {
  const h = harness('footprint');
  assert.match(h.tool.hint!(h.env), /M move.*R rotate.*X mirror/);
  assert.doesNotMatch(h.tool.hint!(h.env), /X route/);
});

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

test('a locked footprint ignores M and R and stays where it is',()=>{
 const h=footprintHarness();h.items.set(4,{...h.items.get(4)!,locked:true} as Item);
 h.move(20,20);h.key('r');h.key('m');h.move(23,22);h.click(23,22);
 assert.deepEqual(h.commands,[]);
 assert.match(useEditor.getState().hint,/Locked/);
});

test('pickUp attaches footprints to the cursor by their centre; click drops them, Esc leaves them',()=>{
 const h=footprintHarness();h.move(30,30);h.tool.pickUp!(h.env,[4]);
 assert.deepEqual(h.selected(),[4]);
 h.move(25,22);h.click(25,22);
 assert.deepEqual(h.commands,[{op:'move',ids:[4],dx:5e6,dy:2e6}]);
 const g=footprintHarness();g.move(30,30);g.tool.pickUp!(g.env,[4]);
 assert.equal(g.tool.cancel(g.env),true);assert.deepEqual(g.commands,[]);
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

test('layer filter: a disabled layer drops its tracks while through vias stay selectable', () => {
  const h = harness(); useSelectFilter.getState().setLayer(0, false);
  h.box(8,0,0,8); assert.deepEqual(h.selected(), [1,2]);
  assert.equal(selectable('pcb', track), false);
  assert.equal(selectable('pcb', { ...track, layer: 1 }), true);
});

test('vias match any copper layer of their span, in stack order', () => {
  useSelectFilter.getState().reset();
  const blind: Item = { ...via(0,0), fromLayer: 0, toLayer: 34 } as Item;
  const f = useSelectFilter.getState();
  f.onlyLayers([1], [0, 33, 34, 1]); assert.equal(selectable('pcb', blind), false);
  f.onlyLayers([33], [0, 33, 34, 1]); assert.equal(selectable('pcb', blind), true);
  f.onlyLayers([1], [0, 33, 34, 1]); assert.equal(selectable('pcb', via(0,0)), true);
});

test('components follow the copper layer of their side', () => {
  const h = footprintHarness(); const f = useSelectFilter.getState();
  f.setLayer(1, false); h.click(20,20); assert.deepEqual(h.selected(), [4]);
  f.setLayer(0, false); h.click(20,20); assert.deepEqual(h.selected(), []);
  f.allLayers(); h.click(20,20); assert.deepEqual(h.selected(), [4]);
});

test('type and layer filters combine, and Reset restores everything', () => {
  const h = harness(); const f = useSelectFilter.getState();
  f.only('tracks'); f.setLayer(0, false);
  h.box(8,0,0,8); assert.deepEqual(h.selected(), []);
  f.reset(); h.box(8,0,0,8); assert.deepEqual(h.selected(), [1,2,3]);
});

test('excluded layers persist as a view preference', () => {
  useSelectFilter.getState().reset(); useSelectFilter.getState().setLayer(33, false);
  assert.deepEqual(JSON.parse(prefs.get('pcbplannercad.selectFilterLayers')!), [33]);
  useSelectFilter.getState().reset();
  assert.deepEqual(JSON.parse(prefs.get('pcbplannercad.selectFilterLayers')!), []);
});

test('Shift+S cycles all → dimmed → active only → all, and only-mode hides and blocks other layers', async () => {
  const { cycleLayerMode } = await import('../src/editor/toolActions.ts');
  const { boardTheme } = await import('../src/editor/theme.ts');
  const h = harness(); const mode = () => useEditor.getState().docs.test.layerMode;
  useEditor.getState().patchDoc('test', { layerMode: 'all' });
  cycleLayerMode('test'); assert.equal(mode(), 'dim');
  cycleLayerMode('test'); assert.equal(mode(), 'only');
  const bottom = { ...track, layer: 1 } as Item;
  assert.equal(selectable('pcb', track), true);
  assert.equal(selectable('pcb', bottom), false);
  assert.equal(selectable('pcb', via(0,0)), true);
  h.box(8,0,0,8); assert.deepEqual(h.selected(), [1,2,3]);
  const only = boardTheme(0, 'only'), dim = boardTheme(0, 'dim');
  assert.equal(only.color('L1')[3], 0); assert.ok(dim.color('L1')[3] > 0);
  assert.ok(only.color('L0')[3] > 0); assert.ok(only.color('ratsnest')[3] > 0);
  cycleLayerMode('test'); assert.equal(mode(), 'all'); assert.equal(selectable('pcb', bottom), true);
});

test('while moving, right click rotates 90° and middle click flips; both commit as one replacement', () => {
  const h = footprintHarness(); h.move(20,20); h.key('m'); h.move(23,22);
  assert.equal(h.tool.auxPress!(h.env, 2, false), true);
  assert.equal(h.tool.auxPress!(h.env, 1, false), true);
  const preview = h.tool.preview!(h.env)[0];
  assert.equal(preview.kind, 'footprint');
  if (preview.kind === 'footprint') { assert.equal(preview.side, 'bottom'); assert.equal(preview.rotation, 270); }
  assert.match(h.tool.hint(h.env), /rotated 90° · flipped/);
  h.click(23,22);
  assert.equal(h.commands.length, 1); const cmd = h.commands[0]; assert.equal(cmd.op, 'replaceMany');
  if (cmd.op === 'replaceMany') { const fp = cmd.items[0].item; if (fp.kind === 'footprint') { assert.equal(fp.side, 'bottom'); assert.equal(fp.rotation, 270); assert.deepEqual(fp.pos, p(23,22)); } }
});

test('Esc discards a flip and rotation made with the mouse buttons', () => {
  const h = footprintHarness(); h.move(20,20); h.key('m'); h.move(24,21);
  h.tool.auxPress!(h.env, 1, false); h.tool.auxPress!(h.env, 2, true);
  assert.equal(h.tool.cancel(h.env), true); assert.deepEqual(h.commands, []); assert.deepEqual(h.tool.preview!(h.env), []);
});

test('outside a move, middle and right buttons are left to panning and the context menu', () => {
  const h = footprintHarness();
  assert.equal(h.tool.auxPress!(h.env, 1, false), false);
  assert.equal(h.tool.auxPress!(h.env, 2, false), false);
});

test('Esc clears a net highlight before it clears the selection', () => {
  const h = harness(); useEditor.getState().patchDoc('test', { marked: new Set([1, 2]), selection: new Set([3]) });
  assert.equal(h.tool.cancel(h.env), true);
  assert.equal(h.env.doc().marked.size, 0); assert.deepEqual(h.selected(), [3]);
  assert.equal(h.tool.cancel(h.env), false, 'the next Esc falls through to deselect');
});

test('dragging a routed track commits neighbours and the move as one routing step', () => {
  const h = harness();
  h.items.set(10, { kind: 'track', a: p(20, 20), b: p(22, 22), width: 200000, layer: 0, net: 'S' });
  h.items.set(11, { kind: 'track', a: p(22, 22), b: p(26, 22), width: 200000, layer: 0, net: 'S' });
  useEditor.getState().patchDoc('test', { items: new Map(h.items) }); h.scene.update(h.items, h.env.ctx());
  h.move(24, 22); h.tool.down!(h.env, { sx: 240, sy: 220, ctrl: false, shift: false, alt: false } as never);
  h.move(24, 23); h.tool.up!(h.env, { sx: 240, sy: 230, ctrl: false, shift: false, alt: false } as never);
  assert.equal(h.commands.length, 1); const cmd = h.commands[0]; assert.equal(cmd.op, 'route');
  if (cmd.op === 'route') {
    assert.deepEqual(cmd.delete, []);
    assert.deepEqual(cmd.replace?.map(r=>r.id).sort(), [10,11], "dragged tracks retain their IDs");
    const moved = cmd.replace?.map(r=>r.item).find((t) => t.kind === 'track' && t.a.y === p(0, 23).y && t.b.y === p(0, 23).y);
    assert(moved, 'the segment moved up 1 mm'); const nb = cmd.replace?.map(r=>r.item).find((t) => t.kind === 'track' && t.a.x === p(20, 0).x);
    assert(nb && nb.kind === 'track' && nb.b.x === p(23, 0).x && nb.b.y === p(0, 23).y, 'the 45° neighbour followed along its own direction');
  }
});

function routedPart() {
  const h = footprintHarness();
  const fp = h.items.get(4) as Extract<Item, { kind: 'footprint' }>;
  h.items.set(4, { ...fp, padNets: { '1': 'SIG' } });
  h.items.set(12, { kind: 'track', a: p(21, 20), b: p(15, 24), width: 200000, layer: 0, net: 'SIG' });
  useEditor.getState().patchDoc('test', { items: new Map(h.items) }); h.scene.update(h.items, h.env.ctx());
  (h.env as { setPlacement: (x: object) => void }).setPlacement = (x) => useEditor.getState().setPlacement(x as never);
  return h;
}

test('moving a routed part previews its track following and commits the backend drag plan', async () => {
  const { mockIPC, clearMocks } = await import('@tauri-apps/api/mocks');
  Object.defineProperty(globalThis, 'window', { configurable: true, value: {} });
  const plan = { replace: [{ id: 4, item: { kind: 'footprint' } }], delete: [12], add: [{ kind: 'track' }], rerouted: 1, blocked: 0 };
  let sent: Record<string, unknown> | null = null;
  mockIPC((cmd, args) => { assert.equal(cmd, 'pcb_drag_footprints'); sent = args!.req as Record<string, unknown>; return plan; });
  try {
    useEditor.getState().setPlacement({ moveMode: 'preserveAngles', moveViolators: 'shove' } as never);
    const h = routedPart(); h.move(20, 20); h.key('m'); h.move(23, 22);
    const stretched = h.tool.preview!(h.env).find((i) => i.kind === 'track');
    assert(stretched && stretched.kind === 'track' && stretched.a.x === p(24, 0).x && stretched.a.y === p(0, 22).y && stretched.b.x === p(15, 0).x, 'the pad end follows, the far end stays');
    assert.match(h.tool.hint(h.env), /preserve angles, push violators \(Tab \/ Shift\+Tab\)/);
    h.click(23, 22); await new Promise((r) => setTimeout(r, 0));
    assert.deepEqual(sent, { ids: [4], delta: p(3, 2), angle: 0, flipped: false, center: p(20,20), mode: 'preserveAngles', routeMode: 'shove', corner: 'diagonal45' });
    assert.deepEqual(h.commands, [{ op: 'route', delete: [12], add: [{ kind: 'track' }], replace: [{ id: 4, item: { kind: 'footprint' } }] }]);
  } finally { clearMocks(); Reflect.deleteProperty(globalThis, 'window'); }
});

test('Tab cycles the move mode; disconnect falls back to a plain move', () => {
  useEditor.getState().setPlacement({ moveMode: 'preserveAngles', moveViolators: 'shove' } as never);
  const h = routedPart(); h.move(20, 20); h.key('m'); h.move(23, 22);
  h.tool.key!(h.env, 'Tab', { shiftKey: true } as KeyboardEvent); assert.equal(useEditor.getState().placement.moveViolators, 'walkaround');
  h.tool.key!(h.env, 'Tab', { shiftKey: false } as KeyboardEvent); assert.equal(useEditor.getState().placement.moveMode, 'free');
  h.tool.key!(h.env, 'Tab', { shiftKey: false } as KeyboardEvent); assert.equal(useEditor.getState().placement.moveMode, 'disconnect');
  assert(!h.tool.preview!(h.env).some((i) => i.kind === 'track'), 'disconnect leaves the track behind');
  h.click(23, 22);
  assert.deepEqual(h.commands, [{ op: 'move', ids: [4], dx: p(3, 0).x, dy: p(0, 2).y }]);
  useEditor.getState().setPlacement({ moveMode: 'preserveAngles', moveViolators: 'shove' } as never);
});


test('exclude locked objects affects selection, preserves layer exclusions and resets with Everything',()=>{
  const h=harness();const f=useSelectFilter.getState();
  const locked={...track,locked:true}as Item;
  assert.equal(selectable('pcb',locked),true);
  f.setLayer(1,false);f.setExcludeLocked(true);
  assert.equal(selectable('pcb',locked),false);
  assert.equal(selectable('pcb',track),true);
  f.onlyTypes(['tracks','vias']);assert.equal(useSelectFilter.getState().excludeLocked,true);assert.deepEqual(useSelectFilter.getState().offLayers,[1]);
  f.reset();assert.equal(useSelectFilter.getState().excludeLocked,false);assert.equal(selectable('pcb',locked),true);
});

test('selection helpers respect hidden layers and hidden object types in the target document',()=>{
  const h=harness();const doc=useEditor.getState().docs.test;
  assert.equal(selectable('pcb',track,{...doc,view:{...doc.view,tracks:false}}),false);
  assert.equal(selectable('pcb',via(0,0),{...doc,view:{...doc.view,vias:false}}),false);
  assert.equal(selectable('pcb',track,{...doc,view:{...doc.view,hiddenLayers:[0]}}),false);
  assert.equal(selectable('pcb',track,doc),true);
});

function routedVia(){
 const h=harness();useEditor.getState().setPlacement({moveMode:'free'});
 h.items.set(12,{kind:'track',a:p(0,3),b:p(3,3),width:200000,layer:0,net:'GND'});
 h.items.set(13,{kind:'track',a:p(3,3),b:p(8,8),width:200000,layer:1,net:'GND'});
 useEditor.getState().patchDoc('test',{items:new Map(h.items)});h.scene.update(h.items,h.env.ctx());return h;
}

test('via pickup previews attached traces and commits one connected route plan',async()=>{
 const {mockIPC,clearMocks}=await import('@tauri-apps/api/mocks');Object.defineProperty(globalThis,'window',{configurable:true,value:{}});
 let requests=0;const plan={replace:[{id:1,item:via(5,4)}],add:[],delete:[],rerouted:0,blocked:0};
 mockIPC((cmd,args)=>{assert.equal(cmd,'pcb_drag_footprints');assert.deepEqual(args!.req.ids,[1]);requests++;return plan;});
 try{const h=routedVia();h.move(3,3);h.key('m');h.move(5,4);
  const preview=h.tool.preview!(h.env);assert(preview.some(i=>i.kind==='track'&&i.b.x===p(5,4).x&&i.b.y===p(5,4).y));
  assert(h.tool.excluded!().has(12)&&h.tool.excluded!().has(13));h.click(5,4);
  await new Promise(r=>setTimeout(r,0));assert.equal(requests,1);assert.equal(h.commands.length,1);assert.equal(h.commands[0].op,'route');
 }finally{clearMocks();}
});

test('cancelling a via drag preserves the board and does not commit',()=>{
 const h=routedVia();const before=h.env.doc().items;h.move(3,3);h.key('m');h.move(5,4);
 assert(h.tool.cancel(h.env));assert.deepEqual(h.commands,[]);assert.deepEqual(h.tool.preview!(h.env),[]);assert.equal(h.env.doc().items,before);
});

test('a late via-move plan cannot overwrite a newer board revision',async()=>{
 const {mockIPC,clearMocks}=await import('@tauri-apps/api/mocks');Object.defineProperty(globalThis,'window',{configurable:true,value:{}});
 let settle!:(value:unknown)=>void;mockIPC(()=>new Promise(resolve=>settle=resolve));
 try{const h=routedVia();h.move(3,3);h.key('m');h.move(5,4);h.click(5,4);
  useEditor.getState().patchDoc('test',{items:new Map(h.env.doc().items)});
  settle({replace:[{id:1,item:via(5,4)}],add:[],delete:[],rerouted:0,blocked:0});await new Promise(r=>setTimeout(r,0));assert.deepEqual(h.commands,[]);
 }finally{clearMocks();}
});

test('curved interior contact preview starts on the curve and ends at the moved via center',()=>{
 const h=harness();useEditor.getState().setPlacement({moveMode:'free'});
 h.items.set(1,via(3,3.2));h.items.set(12,{kind:'arcTrack',center:p(3,-2),radius:5e6,startAngle:0,sweep:180,width:200000,layer:0,net:'GND'});
 useEditor.getState().patchDoc('test',{items:new Map(h.items),view:{...DEFAULT_VIEW,snapToGrid:false}});h.scene.update(h.items,h.env.ctx());
 h.move(3,3.2);h.key('m');h.move(5,4.2);
 const join=h.tool.preview!(h.env).find(i=>i.kind==='track');assert(join&&join.kind==='track');
 assert.deepEqual(join.a,p(3,3));assert.deepEqual(join.b,p(5,4.2),'offset contact reconnects to the via center');
 assert(!h.tool.excluded!().has(12),'the original curve stays visible and fixed');
});

test('whole-curve pickup and mode switches hide only the geometry being moved',()=>{
 const h=harness();useEditor.getState().setPlacement({moveMode:'preserveAngles'});
 (h.env as {setPlacement:(x:object)=>void}).setPlacement=x=>useEditor.getState().setPlacement(x as never);
 h.items.set(1,via(3,3));h.items.set(2,via(-3,3));h.items.set(12,{kind:'arcTrack',center:p(0,3),radius:3e6,startAngle:0,sweep:180,width:200000,layer:0,net:'GND'});
 useEditor.getState().patchDoc('test',{items:new Map(h.items),selection:new Set([1,2])});h.scene.update(h.items,h.env.ctx());
 h.move(0,3);h.key('m');assert(h.tool.excluded!().has(12),'M hides the original curve');
 h.key('Tab');assert(h.tool.excluded!().has(12));h.key('Tab');assert(!h.tool.excluded!().has(12),'Disconnect restores the curve');
 h.key('Tab');assert(h.tool.excluded!().has(12));h.tool.cancel(h.env);assert.equal(h.tool.excluded!().size,0);
});

test('a late move plan cannot clear or commit over a newer via preview',async()=>{
 const {mockIPC,clearMocks}=await import('@tauri-apps/api/mocks');Object.defineProperty(globalThis,'window',{configurable:true,value:{}});
 let settle!:(value:unknown)=>void;mockIPC(()=>new Promise(resolve=>settle=resolve));
 try{const h=routedVia();h.move(3,3);h.key('m');h.move(5,4);h.click(5,4);
  h.move(3,3);h.key('m');h.move(6,5);const preview=h.tool.preview!(h.env);const excluded=[...h.tool.excluded!()];
  settle({replace:[{id:1,item:via(5,4)}],add:[],delete:[],rerouted:0,blocked:0});await new Promise(r=>setTimeout(r,0));
  assert.deepEqual(h.commands,[]);assert.deepEqual(h.tool.preview!(h.env),preview);assert.deepEqual([...h.tool.excluded!()],excluded);
 }finally{clearMocks();}
});
