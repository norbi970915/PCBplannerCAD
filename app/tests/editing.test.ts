import test from 'node:test';
import assert from 'node:assert/strict';
import { bulkCopper, pourOrder, reorderPours } from '../src/pcb/editing.ts';
import { BoundsIndex } from '../src/editor/boundsIndex.ts';
import type { Item } from '../src/core/types.ts';
const track:Item={kind:'track',a:{x:0,y:0},b:{x:1e6,y:0},width:2e5,layer:0,net:'A'};
const via:Item={kind:'via',pos:{x:0,y:0},diameter:6e5,drill:3e5,fromLayer:0,toLayer:1,net:'A'};
const zone=(priority:number,extra:object={}):Item=>({kind:'zone',outline:[],layer:0,net:'A',name:'',priority,clearance:0,minWidth:0,connect:'thermal',thermalGap:0,spokeWidth:0,keepout:false,removeIslands:true,fill:[],...extra});
test('bulk editing leaves mixed, omitted and unrelated properties untouched',()=>{
  const items=new Map<number,Item>([[1,track],[2,{...track,width:3e5}],[3,via]]);
  const changes=bulkCopper(items,[1,2,3],{width:4e5,drill:2e5});
  assert.equal(changes.length,3);assert.equal((changes[0].item as typeof track).width,4e5);
  assert.equal((changes[2].item as typeof via).diameter,6e5);
  assert.equal((changes[2].item as typeof via).drill,2e5);
  assert.equal(items.get(1),track);assert.equal(items.get(3),via);
});
test('invalid mixed via updates are rejected without partial edits',()=>{
  const items=new Map<number,Item>([[1,track],[2,via]]);
  assert.throws(()=>bulkCopper(items,[1,2],{width:5e5,drill:8e5}),/diameter/);
  assert.throws(()=>bulkCopper(items,[2],{toLayer:0}),/different/);
  assert.equal(items.get(1),track);
});
test('locked copper requires explicit unlock and can be locked in bulk',()=>{
  const items=new Map<number,Item>([[1,{...track,locked:true}],[2,via]]);
  assert.throws(()=>bulkCopper(items,[1,2],{width:5e5}),/Unlock/);
  const unlocked=bulkCopper(items,[1],{locked:false});assert.equal((unlocked[0].item as typeof track).locked,false);
  assert.equal(bulkCopper(items,[2],{locked:true}).length,1);
});
test('pour ordering is stable for ties and excludes generated copper and keepouts',()=>{
  const items=new Map<number,Item>([[8,zone(3)],[2,zone(3)],[4,zone(0)],[5,zone(10,{teardrop:true})],[6,zone(20,{keepout:true})]]);
  assert.deepEqual(pourOrder(items),[2,8,4]);
  const changed=reorderPours(items,8,-1);const reordered=new Map(items);for(const r of changed)reordered.set(r.id,r.item);
  assert.deepEqual(pourOrder(reordered),[8,2,4]);assert.equal(reordered.get(5),items.get(5));
  items.set(2,zone(3,{locked:true}));assert.throws(()=>reorderPours(items,8,-1),/Unlock/);
});
test('spatial index matches a linear scan and updates moved/deleted items',()=>{
  const entries=Array.from({length:34000},(_,id)=>({id,bounds:{minX:(id%200)*100,minY:Math.floor(id/200)*100,maxX:(id%200)*100+30,maxY:Math.floor(id/200)*100+30}}));
  const index=new BoundsIndex();index.rebuild(entries);
  for(let q=0;q<20;q++){
    const r={minX:q*113,minY:q*251,maxX:q*113+600,maxY:q*251+800};
    const expected=entries.filter(e=>e.bounds.minX<=r.maxX&&e.bounds.maxX>=r.minX&&e.bounds.minY<=r.maxY&&e.bounds.maxY>=r.minY).map(e=>e.id);
    assert.deepEqual(index.query(r).sort((a,b)=>a-b),expected);
  }
  index.rebuild([{id:7,bounds:{minX:0,minY:0,maxX:5,maxY:5}}]);
  assert.deepEqual(index.query({minX:0,minY:0,maxX:10,maxY:10}),[7]);
});
