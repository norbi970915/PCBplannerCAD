import test from 'node:test';
import assert from 'node:assert/strict';
import {walkCopper,unrouteFootprints,boardStats} from '../src/pcb/boardTools.ts';
import {Scene} from '../src/editor/scene.ts';
import {DEFAULT_VIEW} from '../src/core/view.ts';
import type {Footprint,Item,PlacedFootprintItem} from '../src/core/types.ts';
const pad:Item={kind:'pad',number:'1',pos:{x:0,y:0},rotation:0,shape:'rect',width:2000000,height:2000000,cornerRatio:0,padKind:'tht',layer:15,drill:{width:800000,height:800000},copperShapes:{33:{shape:'circle',width:1200000,height:1200000,cornerRatio:0},0:{shape:'rect',width:2400000,height:2000000,cornerRatio:0}}};
const footprint:Footprint={name:'lands',description:'',model:null,items:[pad]};
const placed:PlacedFootprintItem={kind:'footprint',footprint:'L/P',designator:'J1',value:'',component:'',parameters:{},pos:{x:0,y:0},rotation:0,side:'top',locked:false,padNets:{'1':'P'}};
const view={...DEFAULT_VIEW,padNumbers:false,padNetNames:false,references:false,values:false};
function maxX(scene:Scene){const p=scene.prims(1)!;return Math.max(...p.fills.flatMap(f=>Array.from(f.tris).filter((_,i)=>i%2===0)),...p.segs.flatMap(s=>[s.a.x+s.w/2,s.b.x+s.w/2]));}
test('active-layer switches rebuild cached pad geometry with unchanged item identities',()=>{
 const scene=new Scene(),items=new Map([[1,placed]]);const ctx={kind:'pcb' as const,cache:{},footprints:{'L/P':footprint},view,copperLayer:0};
 scene.update(items,ctx);assert(maxX(scene)>1100000);
 scene.update(items,{...ctx,copperLayer:33});assert(maxX(scene)<700000,'inner circular land must replace outer square');
 scene.update(items,ctx);assert(maxX(scene)>1100000);
});
test('bottom placement flips outer-layer overrides while inner layers keep their identities',()=>{
 const scene=new Scene(),items=new Map([[1,{...placed,side:'bottom' as const}]]);const ctx={kind:'pcb' as const,cache:{},footprints:{'L/P':footprint},view,copperLayer:1};
 scene.update(items,ctx);assert(maxX(scene)>1100000);
 scene.update(items,{...ctx,copperLayer:33});assert(maxX(scene)<700000);
});

test('library pad display uses the selected copper land',()=>{
 const scene=new Scene(),items=new Map([[1,pad]]);const ctx={kind:'footprint' as const,cache:{},view,copperLayer:0};
 scene.update(items,ctx);assert(maxX(scene)>1100000);
 scene.update(items,{...ctx,copperLayer:33});assert(maxX(scene)<700000);
});
const trace=(layer:number,a:number,b:number):Item=>({kind:'track',layer,net:'P',width:100000,a:{x:a,y:0},b:{x:b,y:0}});
test('connection walking and unroute use inner lands while drill statistics count physical pads once',()=>{
 const geometry={'L/P':footprint};
 const items=(layer:number)=>new Map<number,Item>([[1,placed],[2,trace(layer,800000,3000000)],[3,trace(layer,800000,5000000)]]);
 assert.deepEqual([...walkCopper(items(0),geometry,[2],'chain')],[2]);
 assert.deepEqual([...walkCopper(items(33),geometry,[2],'chain')],[2,3]);
 assert.deepEqual(unrouteFootprints(items(33),geometry,[1]),[]);
 assert.deepEqual(new Set(unrouteFootprints(items(0),geometry,[1])),new Set([2,3]));
 const stats=boardStats(items(33),geometry,[]);assert.equal(stats.pads.tht,1);assert.equal(stats.drills[0].count,1);
});
test('bottom connection walking flips outer land dimensions',()=>{
 const items=(layer:number)=>new Map<number,Item>([[1,{...placed,side:'bottom'}],[2,trace(layer,1100000,3000000)],[3,trace(layer,1100000,5000000)]]);
 // The wide land is 2.4 by 2 mm, so use the unroute extent (maximum dimension).
 assert.deepEqual(new Set(unrouteFootprints(items(1),{'L/P':footprint},[1])),new Set([2,3]));
 assert.deepEqual(unrouteFootprints(items(0),{'L/P':footprint},[1]),[]);
});

test('layer changes preserve cached tracks, zones and footprints without layer lands',()=>{
 const scene=new Scene(),plain={...footprint,items:[{...pad,copperShapes:undefined}]};
 const items=new Map<number,Item>([[1,placed],[2,trace(0,3000000,5000000)],[3,{...placed,footprint:'L/plain',designator:'J2'}]]);
 const ctx={kind:'pcb' as const,cache:{},footprints:{'L/P':footprint,'L/plain':plain},view,copperLayer:0};
 scene.update(items,ctx);const track=scene.prims(2),constant=scene.prims(3),land=scene.prims(1),styles=scene.styleCount;
 scene.update(items,{...ctx,copperLayer:33});
 assert.equal(scene.prims(2),track);assert.equal(scene.prims(3),constant);assert.notEqual(scene.prims(1),land);assert.equal(scene.styleCount,styles);
 assert(maxX(scene)<700000);
});

test('round copper uses analytic circles with exact picking and touching-box boundaries',()=>{
 const via:Item={kind:'via',pos:{x:0,y:0},diameter:2000000,drill:600000,fromLayer:0,toLayer:1,net:'P'};
 const scene=new Scene();scene.update(new Map([[1,via]]),{kind:'pcb',cache:{},view});
 assert.equal(scene.prims(1)!.fills.length,0);assert.equal(scene.prims(1)!.segs.length,2);
 assert.equal(scene.pick({x:950000,y:0},0),1);assert.equal(scene.pick({x:800000,y:800000},0),null);
 assert.deepEqual(scene.inRect({minX:950000,minY:-10000,maxX:1050000,maxY:10000},true),[1]);
 assert.deepEqual(scene.inRect({minX:800000,minY:800000,maxX:900000,maxY:900000},true),[]);
 scene.update(new Map([[1,via]]),{kind:'pcb',cache:{},view,smoothCopper:false});assert.equal(scene.prims(1)!.fills.length,2);
});
