import { buildPrint, type PrintOptions } from '../src/pcb/print.ts';
import { drawItem } from '../src/editor/draw.ts';
import type { DocState } from '../src/core/store.ts';
import test from 'node:test';
import assert from 'node:assert/strict';
import { PourTriangles, type FillRequest, type FillWorker } from '../src/editor/pourTriangles.ts';
import { Scene } from '../src/editor/scene.ts';
import { inTriangles, triangulateWithHoles } from '../src/editor/shapes.ts';
import { DEFAULT_VIEW } from '../src/core/view.ts';
import type { Item, ZonePolygon } from '../src/core/types.ts';

const p=(x:number,y:number)=>({x:Math.round(x*1e6),y:Math.round(y*1e6)});
const large=():ZonePolygon=>({outer:Array.from({length:1000},(_,i)=>p(10+5*Math.cos(i*Math.PI/500),10+5*Math.sin(i*Math.PI/500))),holes:[]});
const small=():ZonePolygon=>({outer:[p(0,0),p(10,0),p(10,10),p(0,10)],holes:[[p(2,2),p(4,2),p(4,4),p(2,4)]]});
class FakeWorker implements FillWorker {
 onmessage:FillWorker['onmessage']=null;onerror:FillWorker['onerror']=null;sent:FillRequest[]=[];terminated=0;
 postMessage(message:FillRequest){this.sent.push(message);}
 terminate(){this.terminated++;}
 reply(id:number,triangles=new Float64Array([0,0,1,0,0,1]).buffer){this.onmessage?.({data:{id,triangles}});}
}
function harness(){
 const workers:FakeWorker[]=[],callbacks=new Map<number,()=>void>(),busy:boolean[]=[];let next=1,notifications=0,fallbacks=0;
 const cache=new PourTriangles(()=>notifications++,{worker:()=>{const worker=new FakeWorker();workers.push(worker);return worker;},schedule:cb=>{const id=next++;callbacks.set(id,cb);return id;},cancel:id=>{callbacks.delete(id);},onBusy:value=>busy.push(value),onFallback:()=>fallbacks++});
 return{cache,workers,busy,callbacks,get notifications(){return notifications;},get fallbacks(){return fallbacks;},flush(){const list=[...callbacks.values()];callbacks.clear();for(const cb of list)cb();}};
}

test('large fills prepare once, queue one worker job at a time and coalesce redraws',()=>{
 const h=harness(),a=large(),b=large(),before=JSON.stringify(a);
 assert.equal(h.cache.read(a),null);assert.equal(h.cache.read(a),null);assert.equal(h.cache.read(b),null);
 assert.equal(h.cache.pendingCount,2);assert.equal(h.workers.length,1);assert.equal(h.workers[0].sent.length,1);
 const first=h.workers[0].sent[0].id;h.workers[0].reply(first);
 assert.equal(h.workers[0].sent.length,2);const result=h.cache.read(a);assert(result instanceof Float64Array);assert.deepEqual([...result],[0,0,1,0,0,1]);
 assert.equal(h.cache.read(a),result);h.workers[0].reply(h.workers[0].sent[1].id);
 assert.equal(h.cache.pendingCount,0);assert.equal(h.callbacks.size,1);h.flush();assert.equal(h.notifications,1);
 assert.deepEqual(h.busy,[true,false]);assert.equal(JSON.stringify(a),before,'source polygon remains immutable');h.cache.dispose();
});

test('replaced or hidden fills cancel work and late responses cannot populate newer geometry',()=>{
 const h=harness(),a=large(),b=large();h.cache.read(a);h.cache.read(b);const old=h.workers[0],oldId=old.sent[0].id;
 h.cache.retain(new Set([b]));assert.equal(old.terminated,1);assert.equal(h.workers.length,2);assert.equal(h.workers[1].sent[0].polygon,b);
 old.reply(oldId);assert.equal(h.cache.peek(a),null);assert.equal(h.notifications,0);
 h.workers[1].reply(h.workers[1].sent[0].id);h.flush();assert(h.cache.peek(b) instanceof Float64Array);
 h.cache.retain(new Set([a,b]));h.cache.read(a);assert.notEqual(h.workers[1].sent.at(-1)!.id,oldId,'undo can request the old immutable fill again');
 h.cache.retain(new Set());assert.equal(h.cache.pendingCount,0);assert.equal(h.workers[1].terminated,1);
 assert.equal(h.cache.read(a),null);assert.equal(h.workers.length,2,'hidden data does not restart jobs');h.cache.dispose();
});

test('previews do not enqueue large transformed fills, while small fills remain synchronous',()=>{
 const h=harness();assert.equal(h.cache.peek(large()),null);assert.equal(h.cache.pendingCount,0);assert.equal(h.workers.length,0);
 const polygon=small();assert.deepEqual(h.cache.read(polygon),triangulateWithHoles(polygon.outer,polygon.holes));assert.equal(h.workers.length,0);h.cache.dispose();
});

test('worker errors and malformed buffers fall back without losing filled copper',()=>{
 for(const failure of ['event','buffer']as const){
  const h=harness(),polygon=large();h.cache.read(polygon);
  if(failure==='event')h.workers[0].onerror?.({preventDefault:()=>{}});else h.workers[0].reply(h.workers[0].sent[0].id,new ArrayBuffer(8));
  assert.equal(h.fallbacks,1);assert.equal(h.cache.pendingCount,0);assert.equal(h.workers[0].terminated,1);
  const result=h.cache.read(polygon);assert(result&&result.length>0);assert.deepEqual(result,triangulateWithHoles(polygon.outer,polygon.holes));h.flush();assert.equal(h.notifications,1);h.cache.dispose();
 }
});

test('unavailable workers use the standard path and disposal cancels outstanding callbacks',()=>{
 const callbacks:(()=>void)[]=[];let fallback=0;
 const cache=new PourTriangles(()=>{}, {worker:()=>{throw Error('Unavailable');},schedule:cb=>{callbacks.push(cb);return 1;},cancel:()=>{},onFallback:()=>fallback++});
 const polygon=large();assert.equal(cache.read(polygon),null);assert.equal(fallback,1);assert(cache.read(polygon)!.length>0);cache.dispose();
 const h=harness(),a=large();h.cache.read(a);const worker=h.workers[0];worker.reply(worker.sent[0].id);assert.equal(h.callbacks.size,1);
 h.cache.dispose();assert.equal(h.callbacks.size,0);worker.reply(worker.sent[0].id);h.flush();assert.equal(h.notifications,0);assert.equal(h.cache.read(large()),null);
});

test('pending pours keep hole-aware point and box selection before typed triangles arrive',()=>{
 const polygon=small(),zone:Item={kind:'zone',outline:polygon.outer,fill:[polygon],layer:0,net:'GND',name:'',priority:0,clearance:0,minWidth:0,connect:'solid',thermalGap:0,spokeWidth:0,keepout:false,removeIslands:false};
 const track:Item={kind:'track',a:p(12,0),b:p(14,0),width:200000,layer:0,net:'GND'};
 const items=new Map<number,Item>([[1,zone],[2,track]]),scene=new Scene();let triangles:Float64Array|null=null;
 const context={kind:'pcb' as const,cache:{},footprints:{},view:DEFAULT_VIEW,getFillTriangles:()=>triangles};scene.update(items,context);
 const trackPrims=scene.prims(2),bounds=scene.bounds();assert.equal(scene.prims(1)!.fills.length,0);assert.equal(scene.prims(1)!.pendingFills?.length,1);
 assert.equal(scene.pick(p(6,6),10000),1);assert.equal(scene.pick(p(3,3),10000),null);assert.equal(scene.distance(1,p(2,3)),0,'hole boundary counts as copper');
 const box=(x:number,y:number)=>({minX:p(x,y).x,minY:p(x,y).y,maxX:p(x+.1,y+.1).x,maxY:p(x+.1,y+.1).y});
 assert.deepEqual(scene.inRect(box(6,6),true),[1]);assert.deepEqual(scene.inRect(box(3,3),true),[]);
 triangles=new Float64Array(triangulateWithHoles(polygon.outer,polygon.holes));scene.update(items,context);
 assert.equal(scene.prims(2),trackPrims);assert.equal(scene.prims(1)!.pendingFills,undefined);assert.equal(scene.prims(1)!.fills[0].tris,triangles);assert.deepEqual(scene.bounds(),bounds);
 assert.equal(scene.pick(p(6,6),10000),1);assert.equal(scene.pick(p(3,3),10000),null);assert.deepEqual(scene.inRect(box(3,3),true),[]);
 const readyPrims=scene.prims(1);scene.update(items,context);assert.equal(scene.prims(1),readyPrims);
});


test('print geometry stays identical after a pour has been cached as transferred doubles',()=>{
 const polygon=small(),zone:Item={kind:'zone',outline:polygon.outer,fill:[polygon],layer:0,net:'GND',name:'',priority:0,clearance:0,minWidth:0,connect:'solid',thermalGap:0,spokeWidth:0,keepout:false,removeIslands:false};
 const triangles=new Float64Array(triangulateWithHoles(polygon.outer,polygon.holes));
 drawItem(zone,{kind:'pcb',cache:{},getFillTriangles:()=>triangles});
 const document=(item:Item)=>({info:{kind:'pcb',title:'Test'},items:new Map([[1,item]]),cache:{},footprints:{}}as unknown as DocState);
 const options:PrintOptions={paper:'A4',landscape:false,mono:false};
 const typed=new TextDecoder().decode(buildPrint(document(zone),options));
 const synchronous=new TextDecoder().decode(buildPrint(document(structuredClone(zone)),options));
 assert(typed.startsWith('%PDF-'));assert(!typed.includes('NaN'));
 assert.deepEqual(typed.match(/stream\n[\s\S]*?\nendstream/g),synchronous.match(/stream\n[\s\S]*?\nendstream/g),'identical copper and hole geometry');
});


test('ready pours use polygon boundaries for picking without scanning render triangles',()=>{
 const polygon=small(),zone:Item={kind:'zone',outline:polygon.outer,fill:[polygon],layer:0,net:'GND',name:'',priority:0,clearance:0,minWidth:0,connect:'solid',thermalGap:0,spokeWidth:0,keepout:false,removeIslands:false};
 let vertexReads=0;const triangles=new Float64Array(triangulateWithHoles(polygon.outer,polygon.holes));
 const buffer=new Proxy(triangles,{get(target,key){if(typeof key==='string'&&/^\d+$/.test(key))vertexReads++;return Reflect.get(target,key,target);}});
 const scene=new Scene();scene.update(new Map([[1,zone]]),{kind:'pcb',cache:{},view:DEFAULT_VIEW,getFillTriangles:()=>buffer});
 assert.equal(scene.distance(1,p(6,6)),0);assert(scene.distance(1,p(3,3))>0);
 assert.equal(scene.pick(p(6,6),1000),1);assert.equal(scene.pick(p(3,3),1000),null);
 assert.deepEqual(scene.inRect({minX:5e6,minY:5e6,maxX:6e6,maxY:6e6},true),[1]);
 assert.deepEqual(scene.inRect({minX:2.5e6,minY:2.5e6,maxX:3.5e6,maxY:3.5e6},true),[]);
 assert.equal(vertexReads,0,'bounds and cursor work stay independent of triangle count');
});


test('concave pour picking agrees with copper triangles across winding and multiple holes',()=>{
 const outer=[p(0,0),p(10,0),p(10,4),p(4,4),p(4,10),p(0,10)],holes=[[p(1,1),p(3,1),p(3,3),p(1,3)],[p(.5,5),p(2.5,5),p(2.5,7),p(.5,7)]];
 for(const reverse of [false,true]){
  const polygon={outer:reverse?[...outer].reverse():outer,holes:holes.map(ring=>reverse?[...ring].reverse():ring)};
  const tris=triangulateWithHoles(polygon.outer,polygon.holes);
  const zone:Item={kind:'zone',outline:[p(-1,-1),p(11,-1),p(11,11),p(-1,11)],fill:[polygon],layer:0,net:'GND',name:'',priority:0,clearance:0,minWidth:0,connect:'solid',thermalGap:0,spokeWidth:0,keepout:false,removeIslands:false};
  const scene=new Scene();scene.update(new Map([[1,zone]]),{kind:'pcb',cache:{},view:DEFAULT_VIEW});
  for(let x=0;x<=10;x+=.5)for(let y=0;y<=10;y+=.5)assert.equal(scene.distance(1,p(x,y))===0,inTriangles(p(x,y),tris),'copper containment must match for concavities, holes and boundary points');
 }
});

test('a crossing box selects copper beyond a hole even when its minimum corner is inside the hole',()=>{
 for(const deferred of [false,true]){
  const polygon=small(),zone:Item={kind:'zone',outline:polygon.outer,fill:[polygon],layer:0,net:'GND',name:'',priority:0,clearance:0,minWidth:0,connect:'solid',thermalGap:0,spokeWidth:0,keepout:false,removeIslands:false};
  const scene=new Scene();scene.update(new Map([[1,zone]]),{kind:'pcb',cache:{},view:DEFAULT_VIEW,...(deferred?{getFillTriangles:()=>null}:{})});
  assert.deepEqual(scene.inRect({minX:3e6,minY:3.9e6,maxX:3.1e6,maxY:4.1e6},true),[1]);
 }
});
