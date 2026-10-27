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
  await withIPC((cmd, args) => {
    if (cmd === 'pcb_via') {
      assert.deepEqual(args, { id: 'board', net: 'GND', pos: req.end });
      return via;
    }
    assert.equal(cmd, 'doc_execute');
    assert.deepEqual(commandItems(args!.cmd as Command), [{ ...via, kind: 'via' }]);
  }, async () => {
    const item = await api.pcbVia('board', 'GND', req.end);
    await api.execute('board', { op: 'add', items: [item] });
  });
});

const preferences = new Map<string,string>();
Object.defineProperty(globalThis, 'localStorage', {configurable:true,value:{getItem:(k:string)=>preferences.get(k)??null,setItem:(k:string,v:string)=>preferences.set(k,v),removeItem:(k:string)=>preferences.delete(k)}});
const { routeTool, routingAnchor } = await import('../src/editor/tools/route.ts');
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
    assert.equal(h.commands.length, 1);
    assert(h.commands.every(cmd => cmd.op !== 'route' || cmd.add.every(item => item.kind !== 'via')));
  });
});
