import test from 'node:test';
import assert from 'node:assert/strict';
import {mockIPC,clearMocks} from '@tauri-apps/api/mocks';
import type {DocInfo,Item,ProjectInfo,Snapshot} from '../src/core/types.ts';
Object.defineProperty(globalThis,'localStorage',{configurable:true,value:{getItem:()=>null,setItem:()=>{}}});
Object.defineProperty(globalThis,'window',{configurable:true,value:{}});
const {useEditor}=await import('../src/core/store.ts');
const {linkedTargets,revealLinkedComponent}=await import('../src/core/crossProbe.ts');
const {bus}=await import('../src/core/bus.ts');
const info=(id:string,kind:DocInfo['kind']):DocInfo=>({id,kind,title:id,path:null,dirty:true,canUndo:true,canRedo:false,library:null,entry:null,unit:null,inProject:true});
const part=(ref='R1'):Item=>({kind:'part',pos:{x:0,y:0},rotation:0,mirror:false,libRef:'part',unit:0,fields:[{name:'Designator',value:ref,offset:{x:0,y:0},height:1e6,rotation:0,hAlign:'left',vAlign:'top',visible:true}]});
const fp=(ref='R1'):Item=>({kind:'footprint',designator:ref,value:'10k',footprint:'resistor',pos:{x:50e6,y:20e6},rotation:0,side:'bottom',locked:false,padNets:{},component:'part',parameters:{}});
const snapshot=(id:string,kind:DocInfo['kind'],items:Item[]):Snapshot=>({info:info(id,kind),items:items.map((item,id)=>({id:id+1,item})),cache:{},footprints:{}});
const project:ProjectInfo={path:'A:/Design/Controller.arproj',name:'Controller',sheets:[{file:'Main.arsch',docId:'sch'}],pcb:{file:'Board.arpcb',docId:'board'},libraries:[],variants:[]};
function setup({refs=['R1'],loadBoard=true,reference='R1'}={}){
 useEditor.setState({docs:{},project,messages:[],activeDocId:'sch'});
 useEditor.getState().loadDoc(snapshot('sch','schematic',[part(reference)]));
 if(loadBoard)useEditor.getState().loadDoc(snapshot('board','pcb',refs.map(r=>fp(r))));
 useEditor.getState().loadDoc(snapshot('unrelated','pcb',[fp()]));
}
test('linked targets exclude standalone documents and unrelated boards with identical references',()=>{
 setup();const s=useEditor.getState();assert.deepEqual(linkedTargets(project,s.docs.sch),['board']);assert.deepEqual(linkedTargets(project,s.docs.board),['sch']);assert.deepEqual(linkedTargets(project,s.docs.unrelated),[]);assert.deepEqual(linkedTargets(null,s.docs.sch),[]);
});
test('reveal selects and fits the linked bottom-side footprint without editing data or Undo',async()=>{
 setup();useEditor.getState().setView('board',{hiddenLayers:[0,1,15,14],pads:false});
 const before=useEditor.getState().docs.board,unrelated=useEditor.getState().docs.unrelated;const opened:string[]=[];const off=bus.on('showDoc',e=>opened.push(e.docId));
 try{assert.equal(await revealLinkedComponent('sch',1),true);}finally{off();}
 const after=useEditor.getState().docs.board;
 assert.deepEqual(opened,['board']);assert.equal(useEditor.getState().activeDocId,'board');
 assert.deepEqual([...after.selection],[1]);assert.deepEqual(after.focus?.ids,[1]);assert.equal(after.activeLayer,1);
 assert.deepEqual(after.view.hiddenLayers,[0,14]);assert.equal(after.view.pads,true);
 assert.equal(after.items,before.items);assert.equal(after.info,before.info);assert.equal(useEditor.getState().docs.unrelated,unrelated);
});
test('unplaced, unannotated and duplicate references do not pick an arbitrary footprint',async()=>{
 for(const options of [{refs:['R2']},{refs:['R1','R1']},{reference:'R?'}]){
   setup(options);assert.equal(await revealLinkedComponent('sch',1),false);assert.equal(useEditor.getState().activeDocId,'sch');assert.match(useEditor.getState().hint,/no placed|duplicate|Annotate/);
 }
 setup();useEditor.getState().setProject(null);assert.equal(await revealLinkedComponent('sch',1),false);
 setup();useEditor.getState().setProject({...project,pcb:null});assert.equal(await revealLinkedComponent('sch',1),false);assert.match(useEditor.getState().hint,/unavailable/);
});
test('closed board tab can be loaded and revealed from the native project document',async()=>{
 setup({loadBoard:false});mockIPC((command,args)=>{assert.equal(command,'doc_snapshot');assert.equal(args?.id,'board');return snapshot('board','pcb',[fp()]);});
 try{assert.equal(await revealLinkedComponent('sch',1),true);assert.equal(useEditor.getState().activeDocId,'board');}finally{clearMocks();}
});
test('a project switch rejects a stale board snapshot and does not steal focus',async()=>{
 setup({loadBoard:false});let finish!:(s:Snapshot)=>void;mockIPC(()=>new Promise<Snapshot>(resolve=>{finish=resolve;}));
 try{const pending=revealLinkedComponent('sch',1);useEditor.getState().setProject({...project,path:'A:/Other.arproj'});finish(snapshot('board','pcb',[fp()]));assert.equal(await pending,false);assert(!useEditor.getState().docs.board);assert.equal(useEditor.getState().activeDocId,'sch');}finally{clearMocks();}
});
test('failed board loading preserves the schematic and reports the failure',async()=>{
 setup({loadBoard:false});mockIPC(()=>{throw Error('board missing');});try{assert.equal(await revealLinkedComponent('sch',1),false);assert.equal(useEditor.getState().activeDocId,'sch');assert.match(useEditor.getState().hint,/board missing/);}finally{clearMocks();}
});
test('selection highlights only linked documents, in both directions, and clears within that project',async()=>{
 setup();const {installCrossProbe}=await import('../src/core/pcbActions.ts');const off=installCrossProbe();
 try{
  const unrelated=useEditor.getState().docs.unrelated;
  useEditor.getState().patchDoc('sch',{selection:new Set([1])});
  assert.deepEqual([...useEditor.getState().docs.board.marked],[1]);
  assert.equal(useEditor.getState().docs.unrelated,unrelated);
  useEditor.getState().setActiveDoc('board');
  useEditor.getState().patchDoc('board',{selection:new Set([1])});
  assert.deepEqual([...useEditor.getState().docs.sch.marked],[1]);
  useEditor.getState().patchDoc('board',{selection:new Set()});
  assert.deepEqual([...useEditor.getState().docs.sch.marked],[]);
  useEditor.getState().setActiveDoc('sch');
  useEditor.getState().patchDoc('sch',{selection:new Set()});
  assert.deepEqual([...useEditor.getState().docs.board.marked],[]);
  useEditor.getState().setProject(null);
  useEditor.getState().patchDoc('sch',{selection:new Set([1])});
  assert.deepEqual([...useEditor.getState().docs.board.marked],[]);
 }finally{off();}
});
