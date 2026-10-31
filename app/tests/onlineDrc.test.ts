import test from 'node:test';
import assert from 'node:assert/strict';
import { mergeViolations, touchedBetween } from '../src/pcb/drcMerge.ts';
import { routeModeOf, violatorModeOf } from '../src/core/store.ts';
import type { DrcViolation, Item } from '../src/core/types.ts';

const tr = (x: number, net = 'A'): Item => ({ kind: 'track', a: { x, y: 0 }, b: { x: x + 10, y: 0 }, width: 200, layer: 0, net });
const zone = (fill: number): Item => ({ kind: 'zone', outline: [{ x: 0, y: 0 }, { x: 9, y: 0 }, { x: 9, y: 9 }], layer: 0, net: 'GND', name: '', priority: 0, clearance: 0, minWidth: 0, connect: 'thermal', thermalGap: 0, spokeWidth: 0, keepout: false, removeIslands: true, fill: [{ outer: [{ x: fill, y: 0 }, { x: 9, y: 0 }, { x: 9, y: 9 }], holes: [] }] });
const v = (code: string, items: number[], severity: 'error' | 'warning' = 'error'): DrcViolation => ({ severity, code, rule: '', message: code, items, pos: { x: 0, y: 0 }, layer: 0 });

test('a change re-checks moved and added objects, drops deleted ones, and skips pure refills', () => {
  const a = tr(0), b = tr(50);
  const prev = new Map<number, Item>([[1, a], [2, b], [3, zone(1)], [4, tr(90)]]);
  const next = new Map<number, Item>([[1, a], [2, tr(60)], [3, zone(2)], [5, tr(120)]]);
  const t = touchedBetween(prev, next);
  assert.deepEqual([...t.check].sort(), [2, 5], 'moved track 2 and new track 5; the refilled pour 3 is left alone');
  assert.deepEqual([...t.drop].sort(), [2, 4, 5], 'deleted track 4 loses its markers');
  const moved = new Map(next).set(3, { ...(zone(2) as object), outline: [{ x: 0, y: 0 }, { x: 8, y: 0 }, { x: 8, y: 8 }] } as Item);
  assert.ok(touchedBetween(next, moved).check.has(3), 'a reshaped pour is re-checked');
});

test('markers about changed objects are replaced by the fresh near-DRC, others stay', () => {
  const current = [v('clearance', [2, 7]), v('short', [8, 9]), v('clearance', [4, 6])];
  const fresh = [v('clearance', [2, 7]), v('clearance', [5, 6]), v('unrouted', [5], 'warning')];
  const out = mergeViolations(current, new Set([2, 4, 5]), fresh);
  assert.deepEqual(out.map((x) => `${x.code}:${x.items}`), ['short:8,9', 'clearance:2,7', 'clearance:5,6'], 'deleted track 4 cleared, unrouted left to the ratsnest');
  assert.equal(mergeViolations(out, new Set([2]), [v('clearance', [7, 2])]).length, 3, 'no duplicates when the pair comes back reversed');
});

test('design rules off turns every routing and move mode into ignore', () => {
  const placement = { routeMode: 'shove', moveViolators: 'walkaround' } as never;
  assert.equal(routeModeOf({ drcEnforce: true, placement }), 'shove');
  assert.equal(violatorModeOf({ drcEnforce: true, placement }), 'walkaround');
  assert.equal(routeModeOf({ drcEnforce: false, placement }), 'ignore');
  assert.equal(violatorModeOf({ drcEnforce: false, placement }), 'ignore');
});


test('DRC browser filters severity, type, layer and related net/designator text',async()=>{
  const {filterDrc,drcCsv}=await import('../src/pcb/drcReport.ts');
  const items=new Map<number,Item>([[1,tr(0,'CLOCK')],[2,{kind:'footprint',designator:'U12'}as Item]]);
  const a={...v('clearance',[1]),message:'Too close, use "gap"',rule:'=RULE'},b={...v('width',[2],'warning'),layer:1};
  const filter={query:'',severity:'all' as const,code:'',layer:null};
  assert.deepEqual(filterDrc([a,b],items,{...filter,query:'clock'}),[a]);
  assert.deepEqual(filterDrc([a,b],items,{...filter,query:'u12'}),[b]);
  assert.deepEqual(filterDrc([a,b],items,{...filter,severity:'warning',layer:1}),[b]);
  assert.deepEqual(filterDrc([a,b],items,{...filter,code:'width',layer:0}),[]);
  const csv=drcCsv([a]);assert.ok(csv.startsWith('\uFEFF'));assert.match(csv,/Too close, use ""gap""/);assert.match(csv,/'=RULE/);assert.match(csv,/X \(mm\)/);
});

async function onlineHarness(handler:(cmd:string,args:Record<string,unknown>|undefined)=>unknown,run:(flush:()=>Promise<void>)=>Promise<void>){
  const {mockIPC,clearMocks}=await import('@tauri-apps/api/mocks');
  const {useEditor}=await import('../src/core/store.ts');
  const {installOnlineDrc}=await import('../src/core/pcbActions.ts');
  const original=Object.getOwnPropertyDescriptor(globalThis,'window'),previous=useEditor.getState();
  let timer=0;const callbacks=new Map<number,()=>unknown>();
  Object.defineProperty(globalThis,'window',{configurable:true,value:{setTimeout:(fn:()=>unknown)=>{callbacks.set(++timer,fn);return timer;},clearTimeout:(id:number)=>callbacks.delete(id)}});
  mockIPC(handler as never);
  useEditor.setState({docs:{},activeDocId:'test-drc',onlineDrc:true});
  useEditor.getState().loadDoc({info:{id:'test-drc',kind:'pcb'},items:[{id:1,item:tr(0)}],cache:{},footprints:{}}as never);
  const stop=installOnlineDrc();
  const flush=async()=>{const work=[...callbacks.values()];callbacks.clear();for(const fn of work)fn();await new Promise(resolve=>setImmediate(resolve));};
  try{await run(flush);}finally{stop();clearMocks();useEditor.setState({docs:previous.docs,activeDocId:previous.activeDocId,onlineDrc:previous.onlineDrc});if(original)Object.defineProperty(globalThis,'window',original);else Reflect.deleteProperty(globalThis,'window');}
}
const drcReport=(violations:DrcViolation[]=[])=>({violations,ruleErrors:[],stats:{unrouted:0,connections:0}});

test('online DRC discards stale results and rechecks the full in-flight batch',async()=>{
  const {useEditor}=await import('../src/core/store.ts');
  let resolve!:(value:unknown)=>void,calls=0;
  await onlineHarness(cmd=>{assert.equal(cmd,'pcb_drc');calls++;return calls===1?new Promise(r=>resolve=r):drcReport([v('fresh',[1])]);},async flush=>{
    await flush();assert.equal(useEditor.getState().docs['test-drc'].drcStatus,'checking');
    const doc=useEditor.getState().docs['test-drc'];useEditor.getState().patchDoc('test-drc',{items:new Map(doc.items).set(1,tr(20))});
    resolve(drcReport([v('obsolete',[1])]));await new Promise(r=>setImmediate(r));
    assert.deepEqual(useEditor.getState().docs['test-drc'].drc,[],'obsolete report is never shown');
    await flush();assert.equal(calls,2);assert.equal(useEditor.getState().docs['test-drc'].drcStatus,'current');assert.equal(useEditor.getState().docs['test-drc'].drc[0].code,'fresh');
  });
});

test('turning online DRC off cancels pending work and ignores an in-flight result',async()=>{
  const {useEditor}=await import('../src/core/store.ts');let resolve!:(value:unknown)=>void;
  await onlineHarness(()=>new Promise(r=>resolve=r),async flush=>{
    await flush();useEditor.setState({onlineDrc:false});resolve(drcReport([v('late',[1])]));await flush();
    assert.deepEqual(useEditor.getState().docs['test-drc'].drc,[]);assert.equal(useEditor.getState().docs['test-drc'].drcStatus,'stale');
  });
});

test('a failed online DRC is visible and the next edit retries a full check',async()=>{
  const {useEditor}=await import('../src/core/store.ts');let calls=0;
  await onlineHarness(cmd=>{assert.equal(cmd,'pcb_drc');if(++calls===1)throw Error('temporary check failure');return drcReport();},async flush=>{
    await flush();assert.equal(useEditor.getState().docs['test-drc'].drcStatus,'error');assert.match(useEditor.getState().docs['test-drc'].drcError??'',/temporary/);
    await flush();assert.equal(calls,1,'no uncontrolled retry loop');
    const doc=useEditor.getState().docs['test-drc'];useEditor.getState().patchDoc('test-drc',{items:new Map(doc.items).set(1,tr(30))});await flush();
    assert.equal(calls,2);assert.equal(useEditor.getState().docs['test-drc'].drcStatus,'current');
  });
});


test('incremental checks requeue both in-flight and newer edits instead of losing earlier work',async()=>{
  const {useEditor}=await import('../src/core/store.ts');let settle!:(value:unknown)=>void;const near:number[][]=[];
  await onlineHarness((cmd,args)=>{
    if(cmd==='pcb_drc')return drcReport();
    assert.equal(cmd,'pcb_drc_near');near.push(args!.items as number[]);
    return near.length===1?new Promise(resolve=>settle=resolve):[v('newest',[1,2])];
  },async flush=>{
    await flush();let doc=useEditor.getState().docs['test-drc'];useEditor.getState().patchDoc('test-drc',{items:new Map(doc.items).set(1,tr(20))});await flush();
    doc=useEditor.getState().docs['test-drc'];useEditor.getState().patchDoc('test-drc',{items:new Map(doc.items).set(2,tr(80))});
    settle([v('old',[1])]);await new Promise(resolve=>setImmediate(resolve));await flush();
    assert.deepEqual([...near[1]].sort(),[1,2]);assert.equal(useEditor.getState().docs['test-drc'].drc[0].code,'newest');
  });
});

test('online merging removes old batch-only unrouted markers',()=>{
  const out=mergeViolations([v('unrouted',[8],'warning'),v('clearance',[1])],new Set([1]),[]);
  assert.deepEqual(out,[]);
});


test('manual DRC owns its request while edits queue and a stale manual result forces a full retry',async()=>{
  const {useEditor}=await import('../src/core/store.ts');const {runDrc}=await import('../src/core/pcbActions.ts');
  let calls=0,resolve!:(value:unknown)=>void;
  await onlineHarness(cmd=>{assert.equal(cmd,'pcb_drc','a near-check must not supersede a manual full check');calls++;return calls===2?new Promise(r=>resolve=r):drcReport([v('current',[1])]);},async flush=>{
    await flush();const pending=runDrc('test-drc');
    const doc=useEditor.getState().docs['test-drc'];useEditor.getState().patchDoc('test-drc',{items:new Map(doc.items).set(1,tr(40))});await flush();assert.equal(calls,2);
    resolve(drcReport([v('obsolete',[1])]));await pending;await flush();assert.equal(calls,3);assert.equal(useEditor.getState().docs['test-drc'].drc[0].code,'current');
  });
});

test('turning online checking off does not discard an explicitly requested manual DRC',async()=>{
  const {useEditor}=await import('../src/core/store.ts');const {runDrc}=await import('../src/core/pcbActions.ts');let calls=0,resolve!:(value:unknown)=>void;
  await onlineHarness(()=>++calls===2?new Promise(r=>resolve=r):drcReport(),async flush=>{
    await flush();const pending=runDrc('test-drc');useEditor.setState({onlineDrc:false});resolve(drcReport([v('manual',[1])]));await pending;
    assert.equal(useEditor.getState().docs['test-drc'].drcStatus,'current');assert.equal(useEditor.getState().docs['test-drc'].drc[0].code,'manual');
  });
});

test('global Undo leaves routing while tool-owned undo can preserve the routing session',async()=>{
  const {useEditor}=await import('../src/core/store.ts');const {undo}=await import('../src/core/actions.ts');
  await onlineHarness(cmd=>cmd==='pcb_drc'?drcReport():{info:{id:'test-drc',kind:'pcb',canUndo:true},delta:{removed:[],upserted:[]}},async flush=>{
    await flush();let doc=useEditor.getState().docs['test-drc'];useEditor.getState().patchDoc('test-drc',{tool:'route',info:{...doc.info,canUndo:true}});
    await undo('test-drc');assert.equal(useEditor.getState().docs['test-drc'].tool,'select');
    useEditor.getState().patchDoc('test-drc',{tool:'route'});await undo('test-drc',true);assert.equal(useEditor.getState().docs['test-drc'].tool,'route');
  });
});

test('online updates discard stale whole-net length markers until batch DRC',()=>{assert.deepEqual(mergeViolations([v('length',[1]),v('short',[2,3])],new Set(),[v('length',[4])]).map(x=>x.code),['short']);});

test('manual DRC selects its board inspector while online checks leave View selected',async()=>{
 const {useEditor}=await import('../src/core/store.ts');const {runDrc}=await import('../src/core/pcbActions.ts');const {bus}=await import('../src/core/bus.ts');const requests:unknown[]=[];const off=bus.on('showPanel',request=>requests.push(request));
 try{await onlineHarness(()=>drcReport([v('clearance',[1])]),async flush=>{
  useEditor.getState().patchDoc('test-drc',{inspectorTab:'view'});await flush();assert.equal(useEditor.getState().docs['test-drc'].inspectorTab,'view');assert.deepEqual(requests,[]);
  await runDrc('test-drc');const doc=useEditor.getState().docs['test-drc'];assert.equal(doc.inspectorTab,'drc');assert.equal(doc.drcScope,'full');assert.equal(doc.drcRunId,1);assert.deepEqual(requests,[{id:'properties',docId:'test-drc'}]);
 });}finally{off();}
});
test('failed manual DRC keeps the inspector open and reports the failure',async()=>{
 const {useEditor}=await import('../src/core/store.ts');const {runDrc}=await import('../src/core/pcbActions.ts');let calls=0;
 await onlineHarness(()=>{if(++calls>1)throw Error('Native DRC unavailable');return drcReport();},async flush=>{await flush();useEditor.setState({onlineDrc:false});await runDrc('test-drc');const doc=useEditor.getState().docs['test-drc'];assert.equal(doc.inspectorTab,'drc');assert.equal(doc.drcStatus,'error');assert.match(doc.drcError??'',/unavailable/);});
});

test('manual and online DRC never report a complete check when rule expressions failed',async()=>{
 const {useEditor}=await import('../src/core/store.ts');const {runDrc}=await import('../src/core/pcbActions.ts');
 await onlineHarness(()=>({...drcReport(),ruleErrors:['Clearance: invalid query']}),async flush=>{
  await flush();assert.equal(useEditor.getState().docs['test-drc'].drcStatus,'error');assert.match(useEditor.getState().docs['test-drc'].drcError??'',/incomplete/);
  useEditor.setState({onlineDrc:false});await runDrc('test-drc');const d=useEditor.getState().docs['test-drc'];assert.equal(d.drcStatus,'error');assert.equal(d.drcRuleErrors.length,1);assert.match(d.drcError??'',/rules/);
 });
});