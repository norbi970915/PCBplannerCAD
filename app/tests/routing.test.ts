import test from 'node:test';
import assert from 'node:assert/strict';
import { mockIPC, clearMocks } from '@tauri-apps/api/mocks';
import { api } from '../src/core/api.ts';
import type { Command, RouteRequest } from '../src/core/types.ts';

const req: RouteRequest = {
  net: 'GND', layer: 0, start: { x: 0, y: 0 }, end: { x: 4e6, y: 0 },
  width: 250000, mode: 'walkaround', corner: 'diagonal45', ignore: [], postureFlip: false,
};
// Rust serializes Track and Via structs without the Item enum's `kind` tag.
const track = { a: req.start, b: req.end, width: req.width, layer: 0, net: 'GND' };
const via = { pos: req.end, diameter: 600000, drill: 300000, fromLayer: 0, toLayer: 1, net: 'GND' };
const result = (net = 'GND') => ({ tracks: [{ ...track, net }], moved: [], reached: true, blockedAt: null, message: '' });

async function withIPC(handler: Parameters<typeof mockIPC>[0], run: () => Promise<void>) {
  const original = Object.getOwnPropertyDescriptor(globalThis, 'window');
  Object.defineProperty(globalThis, 'window', { configurable: true, value: {} });
  mockIPC(handler);
  try { await run(); }
  finally {
    clearMocks();
    if (original) Object.defineProperty(globalThis, 'window', original);
    else Reflect.deleteProperty(globalThis, 'window');
  }
}

function commandItems(cmd: Command) {
  if (cmd.op === 'add') return cmd.items;
  if (cmd.op === 'change') return cmd.add;
  throw new Error(`Unexpected command ${cmd.op}`);
}

test('single routing results can be sent straight back as document items', async () => {
  await withIPC((cmd, args) => {
    if (cmd === 'pcb_route') {
      assert.deepEqual(args, { id: 'board', req });
      return result();
    }
    assert.equal(cmd, 'doc_execute');
    assert.deepEqual(commandItems(args!.cmd as Command), [{ ...track, kind: 'track' }]);
  }, async () => {
    const preview = await api.pcbRoute('board', req);
    await api.execute('board', { op: 'add', items: preview.tracks });
  });
});

test('differential pair results tag both nets and preserve routing diagnostics', async () => {
  const raw = [result('USB_P'), { ...result('USB_N'), reached: false, blockedAt: req.end, message: 'Blocked' }];
  await withIPC((cmd, args) => {
    assert.equal(cmd, 'pcb_route_pair');
    assert.deepEqual(args, { id: 'board', req, negNet: 'USB_N', gap: 200000 });
    return raw;
  }, async () => {
    const pair = await api.pcbRoutePair('board', req, 'USB_N', 200000);
    for (let i = 0; i < 2; i++) assert.deepEqual(pair[i], { ...raw[i], tracks: raw[i].tracks.map(t => ({ ...t, kind: 'track' })) });
  });
});

test('shoved replacements retain their IDs and item tags in a routing change', async () => {
  const moved = [{ id: 17, item: { ...track, kind: 'track' as const, net: 'OTHER' } }];
  await withIPC((cmd, args) => {
    if (cmd === 'pcb_route') return { ...result(), moved };
    const change = args!.cmd as Command;
    assert.equal(cmd, 'doc_execute');
    assert.deepEqual(change, { op: 'change', delete: [17], add: [moved[0].item, { ...track, kind: 'track' }] });
  }, async () => {
    const preview = await api.pcbRoute('board', req);
    await api.execute('board', { op: 'change', delete: preview.moved.map(m => m.id), add: [...preview.moved.map(m => m.item), ...preview.tracks] });
  });
});

test('layer-change vias can be committed without a missing kind error', async () => {
  const sent: unknown[] = [];
  await withIPC((cmd, args) => {
    if (cmd === 'pcb_via') {
      sent.push(args);
      return via;
    }
    assert.equal(cmd, 'doc_execute');
    assert.deepEqual(commandItems(args!.cmd as Command), [{ ...via, kind: 'via' }]);
  }, async () => {
    const item = await api.pcbVia('board', 'GND', req.end);
    await api.execute('board', { op: 'add', items: [item] });
    // A layer change passes both layers so the backend can pick a blind or micro via type.
    await api.pcbVia('board', 'GND', req.end, 0, 33);
  });
  assert.deepEqual(sent, [
    { id: 'board', net: 'GND', pos: req.end, from: null, to: null },
    { id: 'board', net: 'GND', pos: req.end, from: 0, to: 33 },
  ]);
});

const preferences = new Map<string,string>();
Object.defineProperty(globalThis, 'localStorage', {configurable:true,value:{getItem:(k:string)=>preferences.get(k)??null,setItem:(k:string,v:string)=>preferences.set(k,v),removeItem:(k:string)=>preferences.delete(k)}});
const routeModule = await import('../src/editor/tools/route.ts');
const { routeTool, routingAnchor } = routeModule;
const { useEditor } = await import('../src/core/store.ts');
const tick = () => new Promise(resolve => setImmediate(resolve));

function toolHarness() {
  let cursor = {x:10200000,y:10000000};
  const fp = {kind:'footprint' as const,pos:{x:10000000,y:10000000},rotation:90,side:'bottom' as const,footprint:'F',padNets:{'1':'SIGNAL'},designator:'U1',value:'',component:'',locked:false,parameters:{}};
  const pad = {kind:'pad' as const,number:'1',pos:{x:1000000,y:0},width:1000000,height:1000000,rotation:0,shape:'rect' as const,padKind:'smd' as const,layer:0,drill:null,cornerRatio:0};
  const doc={items:new Map([[1,fp]]),footprints:{F:{items:[pad]}},activeLayer:1};
  const commands:Command[]=[];
  const env={docId:'board',doc:()=>doc,cursor:()=>cursor,raw:()=>cursor,scale:()=>1e-5,placement:()=>useEditor.getState().placement,redraw:()=>{},exec:async(cmd:Command)=>{commands.push(cmd);return true;}} as unknown as import('../src/editor/tools/types.ts').ToolEnv;
  return {env,commands,tool:routeTool(),move:(x:number,y:number)=>{cursor={x,y};},center:{x:10000000,y:9000000}};
}

test('routing from an off-center click anchors to a rotated bottom pad center', () => {
  const h=toolHarness();assert.deepEqual(routingAnchor(h.env,{x:10200000,y:9300000},[1,'SIGNAL']),h.center);
});

test('interactive pad fanout previews from the exact center and uses an atomic route commit',async()=>{
  const h=toolHarness();
  await withIPC((cmd,args)=>{
    if(cmd==='pcb_object_at')return [1,'SIGNAL'];
    assert.equal(cmd,'pcb_route');const request=args!.req as RouteRequest;
    assert.deepEqual(request.start,h.center);assert.equal(request.net,'SIGNAL');
    return {...result(),tracks:[{...track,a:request.start,b:request.end,layer:1,net:'SIGNAL'}]};
  },async()=>{
    h.tool.down!(h.env,{} as never);await tick();
    h.move(14000000,9000000);h.tool.move!(h.env,{} as never);await tick();
    assert.equal(h.tool.preview!(h.env)[0].kind,'track');
    assert(h.tool.overlay!(h.env).length>0,'the route needs a visible overlay');
    h.tool.down!(h.env,{} as never);await tick();
    assert.equal(h.commands.length,1);assert.equal(h.commands[0].op,'route');
  });
});

test('cancelling while pad lookup is pending cannot restart routing',async()=>{
  const h=toolHarness();let resolveHit!:(value:unknown)=>void;
  await withIPC(cmd=>{assert.equal(cmd,'pcb_object_at');return new Promise(resolve=>{resolveHit=resolve;});},async()=>{
    h.tool.down!(h.env,{} as never);assert.equal(h.tool.cancel(h.env),true);resolveHit([1,'SIGNAL']);await tick();
    assert.deepEqual(h.tool.preview!(h.env),[]);assert.match(h.tool.hint(h.env),/Click a pad/);
  });
});

test('clearance-blocked routing explains why no track can be drawn',async()=>{
  const h=toolHarness();await withIPC(cmd=>cmd==='pcb_object_at'?[1,'SIGNAL']:{...result(),tracks:[],reached:false,blockedAt:h.center,message:'start point violates clearance'},async()=>{
    h.tool.down!(h.env,{} as never);await tick();assert.match(h.tool.hint(h.env),/Blocked: start point violates clearance/);assert(h.tool.overlay!(h.env).length>=4);
  });
});

test('fractional mouse positions are rounded before the native pad lookup',async()=>{
 const h=toolHarness();h.env.raw=()=>({x:10200000.123,y:9300000.987});
 await withIPC((cmd,args)=>{if(cmd==='pcb_object_at'){assert.deepEqual(args!.pos,{x:10200000,y:9300001});return[1,'SIGNAL'];}return{...result(),tracks:[]};},async()=>{h.tool.down!(h.env,{}as never);await tick();});
});


test('a click waits for the latest endpoint instead of committing a stale preview', async () => {
  const h = toolHarness();
  const queued: { request: RouteRequest; resolve: (value: unknown) => void }[] = [];
  await withIPC((cmd, args) => {
    if (cmd === 'pcb_object_at') return [1, 'SIGNAL'];
    assert.equal(cmd, 'pcb_route');
    return new Promise(resolve => queued.push({ request: args!.req as RouteRequest, resolve }));
  }, async () => {
    h.tool.down!(h.env, {} as never); await tick();
    h.move(14000000, 9000000); h.tool.move!(h.env, {} as never);
    h.move(16000000, 9000000); h.tool.down!(h.env, {} as never);
    assert.equal(h.commands.length, 0);
    const settle = (entry: typeof queued[number]) => entry.resolve({ ...result(), tracks: [{ ...track, a: entry.request.start, b: entry.request.end }] });
    settle(queued[0]); await tick();
    assert.deepEqual(queued[1].request.end, { x: 16000000, y: 9000000 });
    settle(queued[1]); await tick();
    assert.equal(h.commands.length, 1);
    const command = h.commands[0];
    assert.equal(command.op, 'route');
    if (command.op === 'route') assert.deepEqual((command.add[0] as typeof track).b, { x: 16000000, y: 9000000 });
  });
});

test('cancelling while a via lookup is pending never inserts the via', async () => {
  const h = toolHarness();
  let resolveVia!: (value: unknown) => void;
  await withIPC(cmd => {
    if (cmd === 'pcb_object_at') return [1, 'SIGNAL'];
    if (cmd === 'pcb_via') return new Promise(resolve => { resolveVia = resolve; });
    return result();
  }, async () => {
    h.tool.down!(h.env, {} as never); await tick();
    h.tool.key!(h.env, 'v', {} as never); await tick();
    assert.equal(typeof resolveVia, 'function');
    h.tool.cancel(h.env); resolveVia(via); await tick();
    assert.equal(h.commands.length, 0);
    assert(h.commands.every(cmd => cmd.op !== 'route' || cmd.add.every(item => item.kind !== 'via')));
  });
});

test('clicking a bottom-side SMD pad while Top is active starts routing on Bottom with its net', async () => {
  const h = toolHarness(); (h.env.doc() as { activeLayer: number }).activeLayer = 0;
  const looked: number[] = []; let routed: RouteRequest | null = null;
  await withIPC((cmd, args) => {
    if (cmd === 'pcb_object_at') { const layer = args!.layer as number; looked.push(layer); return layer === 1 ? [1, 'SIGNAL'] : null; }
    assert.equal(cmd, 'pcb_route'); routed = args!.req as RouteRequest;
    return { ...result(), tracks: [] };
  }, async () => {
    h.tool.down!(h.env, {} as never); await tick(); await tick();
    h.move(14000000, 9000000); h.tool.move!(h.env, {} as never); await tick();
  });
  assert.equal(looked[0], 0, 'the active layer is tried first');
  assert(looked.includes(1));
  assert.match(h.tool.hint(h.env), /Routing SIGNAL on bottom/);
  assert.equal(routed!.layer, 1); assert.equal(routed!.net, 'SIGNAL'); assert.deepEqual(routed!.start, h.center);
});

test('a pad on the active layer wins over an overlapping pad on another layer', async () => {
  const h = toolHarness();
  await withIPC((cmd, args) => cmd === 'pcb_object_at' ? [1, (args!.layer as number) === 1 ? 'SIGNAL' : 'OTHER'] : { ...result(), tracks: [] },
    async () => { h.tool.down!(h.env, {} as never); await tick(); await tick(); });
  assert.match(h.tool.hint(h.env), /Routing SIGNAL on bottom/);
});

test('a plane pour on an inner layer does not take the start away from a bottom pad', async () => {
  const h = toolHarness(); const doc = h.env.doc() as { activeLayer: number; items: Map<number, unknown> };
  doc.activeLayer = 0; doc.items.set(2, { kind: 'zone' });
  doc.items.set(3, { kind: 'board', netClasses: [], stackup: { layers: [0, 33, 34, 1].map(layer => ({ kind: 'copper', layer })) } });
  const looked: number[] = [];
  await withIPC((cmd, args) => {
    if (cmd !== 'pcb_object_at') return { ...result(), tracks: [] };
    const layer = args!.layer as number; looked.push(layer);
    return layer === 33 ? [2, 'GND'] : layer === 1 ? [1, 'SIGNAL'] : null;
  }, async () => {
    (h.env as { doc: () => unknown }).doc = () => ({ ...doc, items: doc.items, activeLayer: doc.activeLayer });
    h.tool.down!(h.env, {} as never); for (let i = 0; i < 6; i++) await tick();
  });
  assert.deepEqual([...new Set(looked)], [0, 33, 34, 1], 'every copper layer is probed in stack order');
  assert.match(h.tool.hint(h.env), /Routing SIGNAL on bottom/);
});

test('clicking open board over a pour on another layer stays on the active layer without a net', async () => {
  const h = toolHarness(); const doc = h.env.doc() as { activeLayer: number; items: Map<number, unknown> };
  doc.activeLayer = 0; doc.items.set(2, { kind: 'zone' });
  await withIPC((cmd, args) => cmd === 'pcb_object_at' ? ((args!.layer as number) === 1 ? [2, 'GND'] : null) : { ...result(), tracks: [] },
    async () => { h.tool.down!(h.env, {} as never); await tick(); await tick(); });
  assert.match(h.tool.hint(h.env), / on top/);
  assert.doesNotMatch(h.tool.hint(h.env), /GND/);
});

test('W and Shift+W step the routing width through the series and net-class widths', () => {
  const { routeWidths } = routeModule;
  const widths = routeWidths({ netClasses: [{ trackWidth: 330000 }] } as never);
  assert.deepEqual(widths.slice(0, 3), [100000, 127000, 150000]);
  assert(widths.includes(330000)); assert.deepEqual(widths, [...widths].sort((a, b) => a - b));
});

test('W widens the route and the status bar shows the new width', async () => {
  const h = toolHarness(); useEditor.getState().setPlacement({ trackWidth: null });
  await withIPC(cmd => cmd === 'pcb_object_at' ? [1, 'SIGNAL'] : { ...result(), tracks: [] }, async () => {
    h.tool.down!(h.env, {} as never); await tick(); await tick();
    (h.env as { setPlacement: (p: object) => void }).setPlacement = (p) => useEditor.getState().setPlacement(p as never);
    assert.match(h.tool.hint(h.env), /width 0\.25 mm/);
    h.tool.key!(h.env, 'w', { shiftKey: false } as KeyboardEvent); assert.match(h.tool.hint(h.env), /width 0\.30 mm/);
    h.tool.key!(h.env, 'W', { shiftKey: true } as KeyboardEvent); h.tool.key!(h.env, 'W', { shiftKey: true } as KeyboardEvent);
    assert.match(h.tool.hint(h.env), /width 0\.20 mm/);
  });
  useEditor.getState().setPlacement({ trackWidth: null });
});

test('net highlight probes the active layer first, marks the net and reports its length', async () => {
  const { highlightNetAt, selectNetCopperAt } = await import('../src/core/pcbActions.ts');
  const items = new Map<number, unknown>([[1, { kind: 'footprint' }], [2, { kind: 'track' }], [3, { kind: 'via' }], [4, { kind: 'zone' }]]);
  useEditor.setState({ docs: { b: { info: { id: 'b', kind: 'pcb' }, items, selection: new Set(), marked: new Set(), activeLayer: 1 } as never }, activeDocId: 'b' });
  const layers: number[] = [];
  await withIPC((cmd, args) => {
    if (cmd === 'pcb_object_at') { layers.push(args!.layer as number); return (args!.layer as number) === 1 ? [1, 'GND'] : null; }
    if (cmd === 'pcb_net_items') { assert.equal(args!.net, 'GND'); return [1, 2, 3, 4]; }
    if (cmd === 'pcb_net_length') return 12_500_000;
    throw new Error(cmd);
  }, async () => {
    await highlightNetAt('b', { x: 1.4, y: 2.6 });
    assert.deepEqual([...useEditor.getState().docs.b.marked], [1, 2, 3, 4]);
    assert.match(useEditor.getState().hint, /Net GND: 4 objects · routed length 12\.50 mm/);
    await selectNetCopperAt('b', { x: 1, y: 2 });
    assert.deepEqual([...useEditor.getState().docs.b.selection], [2, 3], 'only tracks and vias are selected');
  });
  assert.equal(layers[0], 1, 'the active layer is probed first');
});

test('routing lights up the net, rings the other ends and restores the earlier highlight', async () => {
  const h = toolHarness();
  const doc = h.env.doc() as { items: Map<number, unknown>; marked?: Set<number> };
  doc.items.set(2, { kind: 'footprint', pos: { x: 20000000, y: 10000000 }, rotation: 0, side: 'top', footprint: 'F', padNets: { '1': 'SIGNAL' }, designator: 'U2', value: '', component: '', locked: false, parameters: {} });
  const earlier = new Set([99]);
  useEditor.setState({ docs: { board: { ...doc, info: { id: 'board', kind: 'pcb' }, marked: earlier, selection: new Set() } as never } });
  h.env.doc = () => useEditor.getState().docs.board as never;
  await withIPC((cmd) => cmd === 'pcb_object_at' ? [1, 'SIGNAL'] : cmd === 'pcb_net_items' ? [1, 2] : { ...result(), tracks: [] }, async () => {
    h.tool.down!(h.env, {} as never); await tick(); await tick(); await tick();
  });
  assert.deepEqual([...useEditor.getState().docs.board.marked], [1, 2], 'the routed net is highlighted');
  assert.match(h.tool.hint(h.env), /1 other ends/);
  const lines = h.tool.overlay!(h.env);
  const other = { x: 21000000, y: 10000000 };
  const near = (p: { x: number; y: number }, q: { x: number; y: number }) => Math.hypot(p.x - q.x, p.y - q.y) < 2e6;
  assert(lines.some((l) => near(l.a, other) && l.color[2] === 1), 'a cyan ring marks the other pad');
  assert(!lines.some((l) => near(l.a, h.center) && l.color[0] === 0.25 && l.color[3] === 0.95), 'the start pad is not ringed');
  assert(lines.some((l) => l.a.x === h.center.x && l.a.y === h.center.y && l.b.x === other.x && l.b.y === other.y), 'a guide runs to the nearest other end');
  h.tool.cancel(h.env);
  assert.equal(useEditor.getState().docs.board.marked, earlier, 'Esc restores the previous highlight');
});

test('with design rules off the router ignores violators and the hint says so', async () => {
  const h = toolHarness();
  const st = useEditor.getState();
  const modes: string[] = [];
  const route = async () => withIPC((cmd, args) => {
    if (cmd === 'pcb_object_at') return [1, 'SIGNAL'];
    modes.push((args!.req as RouteRequest).mode);
    return { ...result(), tracks: [] };
  }, async () => {
    h.tool.down!(h.env, {} as never); await tick(); await tick();
    h.move(14000000, 9000000); h.tool.move!(h.env, {} as never); await tick();
  });
  if (!st.drcEnforce) st.toggleDrcEnforce();
  st.setPlacement({ routeMode: 'shove' });
  await route();
  h.tool.cancel(h.env);
  useEditor.getState().toggleDrcEnforce();
  await route();
  assert.match(h.tool.hint(h.env), /design rules OFF/);
  useEditor.getState().toggleDrcEnforce();
  assert.deepEqual([modes[0], modes[modes.length - 1]], ['shove', 'ignore']);
});


test('a layer-change via and its exact endpoint tracks commit as one undo transaction',async()=>{
  const h=toolHarness();
  await withIPC((cmd,args)=>{
    if(cmd==='pcb_object_at')return[1,'SIGNAL'];
    if(cmd==='pcb_net_items')return[];
    if(cmd==='pcb_via')return{...via,pos:args!.pos,net:'SIGNAL'};
    const request=args!.req as RouteRequest;
    return{...result('SIGNAL'),tracks:[{...track,a:request.start,b:request.end}]};
  },async()=>{
    h.tool.down!(h.env,{}as never);await tick();h.move(14000000,9000000);
    h.tool.key!(h.env,'v',{}as never);await tick();await tick();
    assert.equal(h.commands.length,1);const command=h.commands[0];assert.equal(command.op,'route');
    if(command.op==='route'){
      assert.equal(command.add.filter(i=>i.kind==='via').length,1);
      assert.deepEqual(command.add.find(i=>i.kind==='via')!.pos,{x:14000000,y:9000000});
      assert.deepEqual(command.add.find(i=>i.kind==='track')!.b,{x:14000000,y:9000000});
    }
    assert.match(h.tool.hint(h.env),/on top/);
  });
});

test('a blocked fanout cannot insert a via at the previous route point',async()=>{
  const h=toolHarness();await withIPC((cmd,args)=>{
    if(cmd==='pcb_object_at')return[1,'SIGNAL'];
    if(cmd==='pcb_net_items')return[];
    if(cmd==='pcb_via')return{...via,pos:args!.pos};
    return{...result(),tracks:[{...track,a:h.center,b:{x:11000000,y:9000000}}],reached:false,blockedAt:h.center,message:'Blocked'};
  },async()=>{
    h.tool.down!(h.env,{}as never);await tick();h.move(14000000,9000000);h.tool.key!(h.env,'v',{}as never);await tick();await tick();
    assert.deepEqual(h.commands,[]);assert.match(h.tool.hint(h.env),/on bottom/);
  });
});

test('a rejected atomic via commit keeps routing on the previous layer',async()=>{
  const h=toolHarness();h.env.exec=async()=>false;
  await withIPC((cmd,args)=>{
    if(cmd==='pcb_object_at')return[1,'SIGNAL'];
    if(cmd==='pcb_net_items')return[];
    if(cmd==='pcb_via')return{...via,pos:args!.pos};
    const request=args!.req as RouteRequest;return{...result(),tracks:[{...track,a:request.start,b:request.end}]};
  },async()=>{
    h.tool.down!(h.env,{}as never);await tick();h.move(14000000,9000000);h.tool.key!(h.env,'v',{}as never);await tick();await tick();
    assert.match(h.tool.hint(h.env),/on bottom/);
  });
});


test('unchanged snapped positions reuse the preview but settings and geometry invalidate it',async()=>{
  const h=toolHarness();let calls=0;
  useEditor.getState().setPlacement({trackWidth:null});
  await withIPC((cmd,args)=>{
    if(cmd==='pcb_object_at')return[1,'SIGNAL'];if(cmd==='pcb_net_items')return[];
    assert.equal(cmd,'pcb_route');calls++;const r=args!.req as RouteRequest;
    return{...result(),tracks:[{...track,a:r.start,b:r.end}]};
  },async()=>{
    h.tool.down!(h.env,{}as never);await tick();await tick();
    for(let i=0;i<100;i++)h.tool.move!(h.env,{}as never);await tick();
    assert.equal(calls,1,'100 duplicate pointer samples must not make 100 native requests');
    useEditor.getState().setPlacement({trackWidth:400000});h.tool.move!(h.env,{}as never);await tick();assert.equal(calls,2);
    h.env.doc().items=new Map(h.env.doc().items);h.tool.move!(h.env,{}as never);await tick();assert.equal(calls,3);
    h.env.doc().footprints={...h.env.doc().footprints};h.tool.move!(h.env,{}as never);await tick();assert.equal(calls,4);
  });
  useEditor.getState().setPlacement({trackWidth:null});
});

test('returning to the in-flight endpoint discards an obsolete queued endpoint',async()=>{
  const h=toolHarness();let calls=0,settle!:(v:unknown)=>void,request!:RouteRequest;
  await withIPC((cmd,args)=>{
    if(cmd==='pcb_object_at')return[1,'SIGNAL'];if(cmd==='pcb_net_items')return[];
    assert.equal(cmd,'pcb_route');calls++;request=args!.req as RouteRequest;
    return new Promise(resolve=>{settle=resolve;});
  },async()=>{
    h.tool.down!(h.env,{}as never);await tick();await tick();
    h.move(14000000,9000000);h.tool.move!(h.env,{}as never);
    h.move(10200000,10000000);h.tool.move!(h.env,{}as never);
    settle({...result(),tracks:[{...track,a:request.start,b:request.end}]});await tick();
    assert.equal(calls,1);assert.deepEqual((h.tool.preview!(h.env)[0] as typeof track).b,request.end);
  });
});

test('Shift+Space cycles corner styles and 0 restores the rule width',async()=>{
  const h=toolHarness();let last!:RouteRequest;
  h.env.setPlacement=patch=>useEditor.getState().setPlacement(patch);
  useEditor.getState().setPlacement({cornerStyle:'diagonal45',trackWidth:400000});
  await withIPC((cmd,args)=>{
    if(cmd==='pcb_object_at')return[1,'SIGNAL'];if(cmd==='pcb_net_items')return[];
    last=args!.req as RouteRequest;return{...result(),tracks:[{...track,a:last.start,b:last.end}]};
  },async()=>{
    h.tool.down!(h.env,{}as never);await tick();await tick();
    for(const style of ['orthogonal90','any','diagonal45']){
      assert.equal(h.tool.key!(h.env,' ',{shiftKey:true}as KeyboardEvent),true);await tick();
      assert.equal(last.corner,style);assert.equal(last.postureFlip,false);
    }
    h.tool.key!(h.env,'/',{shiftKey:false}as KeyboardEvent);await tick();assert.equal(last.postureFlip,true);
    h.tool.key!(h.env,'0',{shiftKey:false}as KeyboardEvent);await tick();
    assert.equal(useEditor.getState().placement.trackWidth,null);assert.equal(last.width,250000);
  });
});

test('a transient preview failure can be retried at the same position',async()=>{
  const h=toolHarness();let calls=0;
  await withIPC((cmd,args)=>{
    if(cmd==='pcb_object_at')return[1,'SIGNAL'];if(cmd==='pcb_net_items')return[];
    if(++calls===1)throw new Error('Temporary routing failure');
    const r=args!.req as RouteRequest;return{...result(),tracks:[{...track,a:r.start,b:r.end}]};
  },async()=>{
    h.tool.down!(h.env,{}as never);await tick();await tick();assert.match(h.tool.hint(h.env),/Temporary routing failure/);
    h.tool.move!(h.env,{}as never);await tick();assert.equal(calls,2);assert.equal(h.tool.preview!(h.env).length,1);
  });
});


test('via routing targets follow stack order, reverse order and explicit copper choices',async()=>{
  const {routeViaLayer}=await import('../src/editor/tools/route.ts');
  const copper=[0,33,34,1];
  assert.equal(routeViaLayer(copper,0,null),33);
  assert.equal(routeViaLayer(copper,0,null,true),1);
  assert.equal(routeViaLayer(copper,33,1),1);
  assert.equal(routeViaLayer(copper,33,33),34,'current layer falls back to the next');
  assert.equal(routeViaLayer(copper,33,999),34,'stale target from another board is ignored');
  assert.equal(routeViaLayer([0],0,null),null);
});

test('Backspace removes a committed routing step and resumes from its previous anchor',async()=>{
  const h=toolHarness();let last!:RouteRequest,undos=0;
  const before=h.env.doc().items;
  h.env.exec=async cmd=>{h.commands.push(cmd);h.env.doc().items=new Map(h.env.doc().items);return true;};
  h.env.undo=async()=>{undos++;h.env.doc().items=new Map(before);return true;};
  await withIPC((cmd,args)=>{
    if(cmd==='pcb_object_at')return[1,'SIGNAL'];if(cmd==='pcb_net_items')return[];
    assert.equal(cmd,'pcb_route');last=args!.req as RouteRequest;
    return{...result(),tracks:[{...track,a:last.start,b:last.end,layer:last.layer}]};
  },async()=>{
    h.tool.down!(h.env,{}as never);await tick();await tick();
    h.move(14000000,9000000);h.tool.down!(h.env,{}as never);await tick();await tick();
    assert.equal(h.commands.length,1);
    assert.equal(h.tool.key!(h.env,'Backspace',{}as never),true);await tick();await tick();
    assert.equal(undos,1);assert.deepEqual(last.start,h.center);
    h.tool.key!(h.env,'Backspace',{}as never);await tick();assert.equal(undos,1,'never undo beyond this route session');
  });
});

test('route Backspace refuses to undo a board change outside this route',async()=>{
  const h=toolHarness();let undos=0;
  h.env.undo=async()=>{undos++;return true;};
  await withIPC((cmd,args)=>{
    if(cmd==='pcb_object_at')return[1,'SIGNAL'];if(cmd==='pcb_net_items')return[];
    const req=args!.req as RouteRequest;return{...result(),tracks:[{...track,a:req.start,b:req.end}]};
  },async()=>{
    h.tool.down!(h.env,{}as never);await tick();await tick();
    h.move(14000000,9000000);h.tool.down!(h.env,{}as never);await tick();await tick();
    h.env.doc().items=new Map(h.env.doc().items);
    h.tool.key!(h.env,'Backspace',{}as never);await tick();
    assert.equal(undos,0);assert.match(h.tool.hint(h.env),/Board changed outside this route/);
  });
});

test('Backspace after a routing via restores its source layer and anchor',async()=>{
  const h=toolHarness();let last!:RouteRequest,undos=0;
  const from=h.env.doc().activeLayer;
  useEditor.getState().setPlacement({routeViaLayer:0});
  h.env.undo=async()=>{undos++;h.env.doc().items=new Map(h.env.doc().items);return true;};
  await withIPC((cmd,args)=>{
    if(cmd==='pcb_object_at')return[1,'SIGNAL'];if(cmd==='pcb_net_items')return[];
    if(cmd==='pcb_via')return{...via,pos:args!.pos,fromLayer:args!.from,toLayer:args!.to};
    last=args!.req as RouteRequest;return{...result(),tracks:[{...track,a:last.start,b:last.end,layer:last.layer}]};
  },async()=>{
    h.tool.down!(h.env,{}as never);await tick();await tick();
    h.move(14000000,9000000);h.tool.key!(h.env,'v',{}as never);await tick();await tick();
    assert.equal(h.commands.length,1);
    h.tool.key!(h.env,'Backspace',{}as never);await tick();await tick();
    assert.equal(undos,1);assert.equal(last.layer,from);assert.deepEqual(last.start,h.center);
  });
  useEditor.getState().setPlacement({routeViaLayer:null});
});


test('pair continuation uses the active-net endpoint even when pair segment counts differ',async()=>{
  const {routeEnd}=await import('../src/editor/tools/route.ts');
  const tracks=[{...track,kind:'track' as const,net:'P',b:{x:1,y:0}},{...track,kind:'track' as const,net:'P',b:{x:2,y:0}},{...track,kind:'track' as const,net:'P',b:{x:3,y:0}},{...track,kind:'track' as const,net:'N',b:{x:3,y:1}}];
  assert.deepEqual(routeEnd(tracks,'P'),{x:3,y:0});assert.deepEqual(routeEnd(tracks,'N'),{x:3,y:1});
});


test('Space opens the copper layer picker and places the via at the captured endpoint',async()=>{
  const h=toolHarness();let pickerCalls=0,viaArgs:Record<string,unknown>|undefined;
  const to={x:14000000,y:9000000};
  h.env.chooseRouteLayer=async options=>{pickerCalls++;assert.equal(options.current,1);assert.deepEqual(options.layers.map(layer=>layer.id),[0,1]);h.move(99000000,99000000);return 0;};
  h.env.dismissRouteLayer=()=>{};
  await withIPC((cmd,args)=>{
    if(cmd==='pcb_object_at')return[1,'SIGNAL'];if(cmd==='pcb_net_items')return[];
    if(cmd==='pcb_via'){viaArgs=args;return{...via,pos:args!.pos,fromLayer:1,toLayer:0};}
    const request=args!.req as RouteRequest;return{...result(),tracks:[{...track,a:request.start,b:request.end,layer:request.layer}]};
  },async()=>{
    h.tool.down!(h.env,{}as never);await tick();await tick();h.move(to.x,to.y);
    h.tool.key!(h.env,' ',{}as never);await tick();await tick();
    assert.equal(pickerCalls,1);assert.deepEqual(viaArgs!.pos,to);assert.equal(viaArgs!.to,0);
    assert.equal(h.commands.length,1);const command=h.commands[0];assert.equal(command.op,'route');if(command.op==='route'){const placed=command.add.find(item=>item.kind==='via');assert.deepEqual(placed?.kind==='via'&&placed.pos,to);}
  });
});

test('cancelling the Space layer picker keeps routing active without adding copper or a via',async()=>{
  const h=toolHarness();let vias=0;
  h.env.chooseRouteLayer=async()=>null;h.env.dismissRouteLayer=()=>{};
  await withIPC((cmd,args)=>{
    if(cmd==='pcb_object_at')return[1,'SIGNAL'];if(cmd==='pcb_net_items')return[];if(cmd==='pcb_via'){vias++;return via;}
    const request=args!.req as RouteRequest;return{...result(),tracks:[{...track,a:request.start,b:request.end}]};
  },async()=>{
    h.tool.down!(h.env,{}as never);await tick();await tick();h.tool.key!(h.env,' ',{}as never);await tick();
    assert.equal(vias,0);assert.equal(h.commands.length,0);assert.match(h.tool.hint(h.env),/Routing SIGNAL/);
  });
});

test('ending the route closes a pending layer picker and ignores its late choice',async()=>{
  const h=toolHarness();let settle!:(layer:number|null)=>void,dismissed=0,vias=0;
  h.env.chooseRouteLayer=()=>new Promise(resolve=>settle=resolve);h.env.dismissRouteLayer=()=>{dismissed++;};
  await withIPC((cmd,args)=>{
    if(cmd==='pcb_object_at')return[1,'SIGNAL'];if(cmd==='pcb_net_items')return[];if(cmd==='pcb_via'){vias++;return via;}
    const request=args!.req as RouteRequest;return{...result(),tracks:[{...track,a:request.start,b:request.end}]};
  },async()=>{
    h.tool.down!(h.env,{}as never);await tick();await tick();h.tool.key!(h.env,' ',{}as never);
    h.tool.cancel(h.env);settle(0);await tick();assert.equal(dismissed,1);assert.equal(vias,0);assert.equal(h.commands.length,0);
  });
});

test('routing commands default to via enforcement and forward explicit rules-off',async()=>{
  const flags:unknown[]=[];
  await withIPC((cmd,args)=>{assert.equal(cmd,'doc_execute');flags.push(args!.enforceVias);},async()=>{
    const command:Command={op:'route',delete:[],add:[{...via,kind:'via'}]};
    await api.execute('board',command);
    await api.execute('board',command,false);
  });
  assert.deepEqual(flags,[true,false]);
});

test('rejected via keeps its source layer and anchor, adds no history, and can be retried',async()=>{
  const h=toolHarness();let reject=true,undos=0,last!:RouteRequest;
  useEditor.setState({docs:{board:h.env.doc()},activeDocId:'board'});
  useEditor.getState().setPlacement({routeViaLayer:0});
  h.env.exec=async command=>{if(reject)return false;h.commands.push(command);return true;};
  h.env.undo=async()=>{undos++;return true;};
  await withIPC((cmd,args)=>{
    if(cmd==='pcb_object_at')return[1,'SIGNAL'];if(cmd==='pcb_net_items')return[];
    if(cmd==='pcb_via')return{...via,pos:args!.pos,fromLayer:1,toLayer:0,net:'SIGNAL'};
    last=args!.req as RouteRequest;
    return{...result('SIGNAL'),tracks:[{...track,a:last.start,b:last.end,layer:last.layer,net:'SIGNAL'}]};
  },async()=>{
    h.tool.down!(h.env,{}as never);await tick();await tick();
    h.move(14000000,9000000);h.tool.key!(h.env,'v',{}as never);await tick();await tick();
    assert.match(h.tool.hint(h.env),/Via placement rejected/);
    assert.equal(useEditor.getState().docs.board.activeLayer,1);
    assert.equal(h.commands.length,0);
    assert.deepEqual(last.start,h.center);
    h.tool.key!(h.env,'Backspace',{}as never);await tick();assert.equal(undos,0);
    reject=false;h.move(18000000,9000000);h.tool.move!(h.env,{}as never);await tick();
    h.tool.key!(h.env,'v',{}as never);await tick();await tick();
    assert.equal(h.commands.length,1);
    if(h.commands[0].op==='route')assert.deepEqual((h.commands[0].add.find(i=>i.kind==='track') as typeof track).a,h.center);
    assert.equal(useEditor.getState().docs.board.activeLayer,0);
    h.tool.cancel(h.env);
  });
  useEditor.getState().setPlacement({routeViaLayer:null});
});

test('standalone via placement uses the same guarded, refilling route transaction',async()=>{
  const {makeTool}=await import('../src/editor/tools/place.ts');
  const h=toolHarness();useEditor.getState().setPlacement({net:'SIGNAL'});
  makeTool('via')!.down!(h.env,{}as never);await tick();
  assert.equal(h.commands.length,1);assert.equal(h.commands[0].op,'route');
  if(h.commands[0].op==='route')assert.equal(h.commands[0].add[0].kind,'via');
});

test('paste, bulk properties and transforms forward the explicit rules setting',async()=>{
  const commands:Command[]=[
    {op:'add',items:[{...via,kind:'via'}]},
    {op:'change',delete:[],add:[{...via,kind:'via'}]},
    {op:'replaceMany',items:[{id:1,item:{...via,kind:'via'}}]},
    {op:'move',ids:[1],dx:1,dy:0},
    {op:'rotate',ids:[1],center:{x:0,y:0},angle:90},
    {op:'mirror',ids:[1],center:{x:0,y:0}},
  ];
  await withIPC((cmd,args)=>{assert.equal(cmd,'doc_execute');assert.equal(args!.enforceVias,false);},async()=>{
    for(const command of commands)await api.execute('board',command,false);
  });
});