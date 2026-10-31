import test from 'node:test';
import assert from 'node:assert/strict';
import type { Item } from '../src/core/types.ts';
import { copperLength, prospectiveLength, routedLengths, targetBounds, lengthBand } from '../src/editor/routeLengths.ts';
const t=(net:string,length:number):Item=>({kind:'track',locked:false,a:{x:0,y:0},b:{x:length,y:0},width:200000,layer:0,net});
test('copper totals include arcs and branches; preview replacements do not double count',()=>{
 const arc:Item={kind:'arcTrack',locked:false,center:{x:0,y:0},radius:1000000,startAngle:0,sweep:-180,width:200000,layer:0,net:'P'};
 const items=new Map([[1,t('P',5e6)],[2,t('P',2e6)],[3,t('N',6e6)],[4,arc]]);
 assert.equal(copperLength(arc),Math.PI*1e6);assert.equal(routedLengths(items),routedLengths(items));
 assert.equal(prospectiveLength(items,'P',[t('P',1e6),t('N',7e6)],[{id:1,item:t('P',6e6)}]),Math.round(9e6+Math.PI*1e6));
 assert.equal(prospectiveLength(items,'N',[],[{id:3,item:t('P',6e6)}]),0);
 const committed=new Map(items);committed.set(5,t('P',1e6));assert.notEqual(routedLengths(committed),routedLengths(items));
 assert.equal(prospectiveLength(committed,'P',[],[]),Math.round(8e6+Math.PI*1e6));
});
test('length gauge bands include boundaries and reject conflicting targets',()=>{
 const bounds={min:10,max:12};assert.equal(lengthBand(9,bounds),'below');assert.equal(lengthBand(10,bounds),'within');assert.equal(lengthBand(12,bounds),'within');assert.equal(lengthBand(13,bounds),'above');
 assert.equal(lengthBand(10,null),'unset');assert.equal(lengthBand(NaN,bounds),'invalid');
 const rule={min:10,max:12,rule:'Signal'};
 assert.deepEqual(targetBounds(rule,12,1),{min:11,max:12});assert.equal(lengthBand(15,targetBounds(rule,15,1)),'invalid');
 assert.deepEqual(targetBounds(null,1,2),{min:0,max:3});assert.equal(targetBounds(rule,null,1),rule);
});

test('invalid manual limits and invalid lengths cannot report an in-range result',()=>{
  for(const [target,tolerance] of [[0,1],[-1,1],[NaN,1],[10,-1],[10,Infinity]]) assert.equal(lengthBand(10,targetBounds(null,target,tolerance)),'invalid');
  assert.equal(lengthBand(NaN,null),'invalid');assert.equal(lengthBand(-1,null),'invalid');
});

test('split shove replacements subtract their original track once and count every new piece',()=>{
  const items=new Map([[1,t('P',5e6)],[2,t('N',6e6)]]);
  const moved=[{id:1,item:t('P',2e6)},{id:1,item:t('P',4e6)},{id:2,item:t('N',3e6)},{id:2,item:t('N',4e6)}];
  assert.equal(prospectiveLength(items,'P',[],moved),6e6);
  assert.equal(prospectiveLength(items,'N',[],moved),7e6);
});

test('via-owned tether previews count new copper without replacing an existing track',()=>{
  const original:Item={kind:'via',locked:false,pos:{x:0,y:0},diameter:600000,drill:300000,fromLayer:0,toLayer:1,net:'P'};
  const items=new Map<number,Item>([[1,t('P',10e6)],[17,original]]);
  const moved=[{id:17,item:{...original,pos:{x:0,y:635000}} as Item},{id:17,item:t('P',635000)}];
  for(const entries of [moved,[...moved].reverse()]) assert.equal(prospectiveLength(items,'P',[],entries),10635000);
  assert.equal(items.get(17),original);
  assert.equal(prospectiveLength(items,'P',[],[]),10e6);
});
