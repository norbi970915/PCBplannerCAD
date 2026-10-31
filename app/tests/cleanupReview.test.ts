import test from 'node:test';
import assert from 'node:assert/strict';
import {mockIPC,clearMocks} from '@tauri-apps/api/mocks';
import {api} from '../src/core/api.ts';
import {cleanupSource,cleanupReviewCurrent,type CleanupReview} from '../src/pcb/cleanupReview.ts';
import type {CleanupOptions,CleanupResult} from '../src/core/types.ts';
const opts:CleanupOptions={zeroLength:true,duplicates:true,dangling:true,unconnectedVias:false,mergeCollinear:true};
const plan:CleanupResult={delete:[],replace:[],original:[],trimmed:0,merged:0,dangling:0,duplicates:0,zeroLength:0,vias:0};
const document=()=>({items:new Map(),footprints:{},epoch:1});
test('cleanup review expires on copper, library, reset, document and option changes',()=>{
  const doc=document();const review:CleanupReview={source:cleanupSource('b',opts,doc)!,plan};
  assert(cleanupReviewCurrent(review,'b',{...opts},doc));
  for(const changed of [{...doc,items:new Map()},{...doc,footprints:{}},{...doc,epoch:2}])
    assert.equal(cleanupReviewCurrent(review,'b',opts,changed),false);
  assert.equal(cleanupReviewCurrent(review,'other',opts,doc),false);
  assert.equal(cleanupReviewCurrent(review,'b',{...opts,dangling:false},doc),false);
  assert.equal(cleanupReviewCurrent(review,'b',opts,undefined),false);
  assert.equal(cleanupReviewCurrent(null,'b',opts,doc),false);
});
test('cleanup captures the source epoch before a delayed native preview returns',()=>{
  const doc=document();const source=cleanupSource('b',opts,doc)!;
  doc.epoch=2;const late={source,plan};
  assert.equal(cleanupReviewCurrent(late,'b',opts,doc),false);
  assert.equal(source.epoch,1);
  const fresh={source:cleanupSource('b',opts,doc)!,plan};
  assert(cleanupReviewCurrent(fresh,'b',opts,doc));
});
test('cleanup option ordering does not invalidate the same reviewed choices',()=>{
  const doc=document();const review={source:cleanupSource('b',opts,doc)!,plan};
  const reordered=Object.fromEntries(Object.entries(opts).reverse())as unknown as CleanupOptions;
  assert(cleanupReviewCurrent(review,'b',reordered,doc));
});
test('cleanup apply sends the reviewed source and options to its native commit command',async()=>{
  const original=Object.getOwnPropertyDescriptor(globalThis,'window');
  Object.defineProperty(globalThis,'window',{configurable:true,value:{}});
  const expected:CleanupResult={...plan,delete:[17],original:[{id:17,item:{kind:'track',locked:false,a:{x:0,y:0},b:{x:1000000,y:0},net:'N',layer:0,width:250000}}]};
  const reply={delta:{upserted:[],deleted:[17]},info:{id:'board'}};
  mockIPC((command,args)=>{assert.equal(command,'pcb_cleanup_apply');assert.deepEqual(args,{id:'board',opts,expected});return reply;});
  try{assert.deepEqual(await api.pcbCleanupApply('board',opts,expected),reply);}
  finally{clearMocks();if(original)Object.defineProperty(globalThis,'window',original);else Reflect.deleteProperty(globalThis,'window');}
});
