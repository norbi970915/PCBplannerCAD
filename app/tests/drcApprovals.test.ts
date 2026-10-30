import {test} from 'node:test';
import assert from 'node:assert/strict';
import {approvalOnlyChange,findingKey,makeDrcApproval,reviewDrc} from '../src/pcb/drcApprovals.ts';
import {drcCsv} from '../src/pcb/drcReport.ts';
import type {BoardItem,DrcViolation,Footprint,Item,ItemId} from '../src/core/types.ts';

const finding:DrcViolation={severity:'error',code:'clearance',rule:'Clearance',message:'Too close',items:[1,2],pos:{x:10,y:20},layer:0};
const board=():BoardItem=>({kind:'board',stackup:{layers:[]},rules:[],netClasses:[],diffPairs:[],origin:{x:0,y:0},maskColor:'green',silkColor:'white'});
const fixture=()=>({items:new Map<ItemId,Item>([[100,board()],[1,{kind:'track',a:{x:0,y:0},b:{x:20,y:0},width:10,layer:0,net:'A'}],[2,{kind:'track',a:{x:0,y:15},b:{x:20,y:15},width:10,layer:0,net:'B'}]]),footprints:{} as Record<string,Footprint>,drc:[finding]});
function approvedFixture(){const doc=fixture(),approval=makeDrcApproval(doc,finding);doc.items.set(100,{...doc.items.get(100) as BoardItem,drcApprovals:[approval]});return{doc,approval};}

test('exact approvals survive serialization and repeat DRC, without discarding raw findings',()=>{
 const {doc,approval}=approvedFixture();let review=reviewDrc(doc);assert.equal(review.open.length,0);assert.equal(review.current.get(finding),approval);assert.equal(doc.drc.length,1);
 const reopened={items:new Map<ItemId,Item>(JSON.parse(JSON.stringify([...doc.items]))),footprints:JSON.parse(JSON.stringify(doc.footprints)),drc:JSON.parse(JSON.stringify(doc.drc))};
 review=reviewDrc(reopened);assert.equal(review.open.length,0);assert.equal(review.approved[0].state,'current');
 assert.equal(findingKey({...finding,items:[2,1,1]}),findingKey(finding));
});
test('approval does not cover changed measurements, severity, position, geometry, removed objects or rules',()=>{
 for(const patch of [{message:'Further violation'},{severity:'warning' as const},{pos:{x:11,y:20}}]){const {doc}=approvedFixture();doc.drc=[{...finding,...patch}];assert.equal(reviewDrc(doc).open.length,1);}
 {const {doc}=approvedFixture();const item=doc.items.get(1)!;doc.items=new Map(doc.items).set(1,{...item,net:'C'} as Item);assert.equal(reviewDrc(doc).open.length,1);assert.equal(reviewDrc(doc).approved[0].state,'changed');}
 {const {doc}=approvedFixture();doc.items=new Map(doc.items);doc.items.delete(2);assert.equal(reviewDrc(doc).open.length,1);}
 {const {doc}=approvedFixture();doc.items=new Map(doc.items).set(100,{...doc.items.get(100) as BoardItem,rules:[{type:'width',name:'New rule',enabled:true,scope:'All',min:1,preferred:2,max:3}]});assert.equal(reviewDrc(doc).open.length,1);}
});
test('unrelated geometry does not invalidate object-specific reviews, but global findings track whole-board geometry',()=>{
 const {doc}=approvedFixture();doc.items=new Map(doc.items).set(3,{...doc.items.get(1)!} as Item);assert.equal(reviewDrc(doc).open.length,0);
 const global=fixture();global.drc=[{...finding,code:'no-outline',items:[]}];const record=makeDrcApproval(global,global.drc[0]);global.items.set(100,{...global.items.get(100) as BoardItem,drcApprovals:[record]});assert.equal(reviewDrc(global).open.length,0);
 global.items=new Map(global.items).set(3,{...global.items.get(1)!} as Item);assert.equal(reviewDrc(global).open.length,1);
});
test('saved approvals stay reviewable when not reported, and removing approval restores a matching finding',()=>{
 const {doc}=approvedFixture();doc.drc=[];assert.equal(reviewDrc(doc).approved[0].state,'notReported');
 doc.drc=[finding];doc.items=new Map(doc.items).set(100,{...doc.items.get(100) as BoardItem,drcApprovals:[]});assert.equal(reviewDrc(doc).open.length,1);
});
test('approval-only updates preserve DRC validity; geometry and other settings do not',()=>{
 const {doc}=approvedFixture(),base=new Map(doc.items).set(100,board());assert.equal(approvalOnlyChange(base,doc.items),true);
 const changed=new Map(doc.items).set(100,{...doc.items.get(100) as BoardItem,origin:{x:4,y:5}});assert.equal(approvalOnlyChange(base,changed),false);
 assert.equal(approvalOnlyChange(base,new Map(base)),false);
 const geometry=new Map(doc.items).set(1,{...doc.items.get(1)!,width:20} as Item);assert.equal(approvalOnlyChange(base,geometry),false);
 const csv=drcCsv([finding],()=> 'Approved');assert.match(csv,/Review status/);assert.match(csv,/"Approved"/);
});
test('changed footprint definitions invalidate component approvals',()=>{
 const doc=fixture();doc.items.set(1,{kind:'footprint',pos:{x:0,y:0},rotation:0,side:'top',footprint:'F',designator:'U1',value:'A',component:'',locked:false,padNets:{},parameters:{}});
 doc.footprints={F:{name:'F',description:'',items:[],model:null}};const approval=makeDrcApproval(doc,finding);doc.items.set(100,{...doc.items.get(100) as BoardItem,drcApprovals:[approval]});assert.equal(reviewDrc(doc).open.length,0);
 const changed={...doc,footprints:{F:{...doc.footprints.F,description:'Changed geometry source'}}};assert.equal(reviewDrc(changed).open.length,1);
});
test('native approval rejection leaves the board unchanged; successful approval, reload and Undo retain correct status',async()=>{
 const {api}=await import('../src/core/api.ts'),{useEditor}=await import('../src/core/store.ts'),{setDrcApproval}=await import('../src/pcb/drcApprovalActions.ts');
 const previous=useEditor.getState(),oldApi=api.pcbDrcApproval;const doc=fixture();
 useEditor.setState({docs:{},activeDocId:'approval-test'});useEditor.getState().loadDoc({info:{id:'approval-test',kind:'pcb',title:'Test',path:null,dirty:false,canUndo:false,canRedo:false,library:null,entry:null,unit:null,inProject:false},items:[...doc.items].map(([id,item])=>({id,item})),footprints:{},cache:{}});
 useEditor.getState().patchDoc('approval-test',{drc:[finding],drcStatus:'current'});
 try{
  let calls=0;api.pcbDrcApproval=async()=>{calls++;throw Error('Write blocked');};
  const originalItems=useEditor.getState().docs['approval-test'].items;
  assert.equal(await setDrcApproval('approval-test',finding,true),false);assert.equal(useEditor.getState().docs['approval-test'].items,originalItems);
  useEditor.getState().patchDoc('approval-test',{drcStatus:'stale'});assert.equal(await setDrcApproval('approval-test',finding,true),false);assert.equal(calls,1);
  useEditor.getState().patchDoc('approval-test',{drcStatus:'current'});
  api.pcbDrcApproval=async(id,record)=>({info:{...useEditor.getState().docs[id].info,dirty:true,canUndo:true},delta:{removed:[],upserted:[{id:100,item:{...board(),drcApprovals:[record]}}]}});
  assert.equal(await setDrcApproval('approval-test',finding,true),true);let current=useEditor.getState().docs['approval-test'];assert.equal(current.drcStatus,'current');assert.equal(reviewDrc(current).open.length,0);assert.equal(current.info.dirty,true);
  useEditor.getState().applyUpdate({info:current.info,delta:{removed:[],upserted:[{id:100,item:board()}]}});current=useEditor.getState().docs['approval-test'];assert.equal(current.drcStatus,'current');assert.equal(reviewDrc(current).open.length,1);
 }finally{api.pcbDrcApproval=oldApi;useEditor.setState(previous);}
});
