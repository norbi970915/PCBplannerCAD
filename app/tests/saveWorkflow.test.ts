import test from 'node:test';
import assert from 'node:assert/strict';
import {mockIPC,clearMocks} from '@tauri-apps/api/mocks';
import type {DocInfo,LibInfo,ProjectInfo} from '../src/core/types.ts';
import {folderNameError} from '../src/project/paths.ts';
const preferences=new Map<string,string>();
Object.defineProperty(globalThis,'localStorage',{configurable:true,value:{getItem:(k:string)=>preferences.get(k)??null,setItem:(k:string,v:string)=>preferences.set(k,v),removeItem:(k:string)=>preferences.delete(k)}});
Object.defineProperty(globalThis,'window',{configurable:true,value:{}});
const {useEditor}=await import('../src/core/store.ts');
const {saveAll,saveDoc,closeDoc,applyProjectState}=await import('../src/core/actions.ts');
const {api}=await import('../src/core/api.ts');
const info=(id:string,extra:Partial<DocInfo>={}):DocInfo=>({id,kind:'schematic',title:id,path:null,dirty:true,canUndo:true,canRedo:false,library:null,entry:null,unit:null,inProject:false,...extra});
const library:LibInfo={id:'library',name:'Assets',path:null,dirty:true,readonly:false,symbols:1,footprints:1,components:1,inProject:false};
const snapshot=(i:DocInfo)=>({info:i,items:[],cache:{},footprints:{}});
function reset(){useEditor.setState({docs:{},libs:[],project:null,messages:[],activeDocId:null});}
async function ipc(handler:Parameters<typeof mockIPC>[0],run:()=>Promise<void>){reset();mockIPC(handler);try{await run();}finally{clearMocks();reset();}}
test('cancelled Save All stays unsaved and never reports complete success',async()=>{await ipc(cmd=>{if(cmd==='project_save_all')return [];if(cmd==='plugin:dialog|save')return null;throw Error(cmd);},async()=>{useEditor.getState().loadDoc(snapshot(info('untitled')));assert.equal(await saveAll(),false);assert.equal(useEditor.getState().docs.untitled.info.dirty,true);assert(!useEditor.getState().messages.some(m=>m.text.includes('Saved all')));});});
test('Save All includes unnamed dirty libraries and refreshes their saved state',async()=>{let saved=false;await ipc((cmd,args)=>{if(cmd==='project_save_all')return [];if(cmd==='lib_list')return [{...library,path:saved?'A:/library.arlib':null,dirty:!saved}];if(cmd==='plugin:dialog|save')return 'A:/library.arlib';if(cmd==='lib_save'){assert.deepEqual(args,{id:'library',path:'A:/library.arlib'});saved=true;return {...library,path:'A:/library.arlib',dirty:false};}if(cmd==='doc_list')return [];throw Error(cmd);},async()=>{assert.equal(await saveAll(),true);assert(saved);assert.equal(useEditor.getState().libs[0].dirty,false);});});
test('failed native Close preserves the live document and active tab',async()=>{await ipc(cmd=>{throw Error(cmd+' reload failed');},async()=>{useEditor.getState().loadDoc(snapshot(info('keep',{dirty:false})));useEditor.getState().setActiveDoc('keep');assert.equal(await closeDoc('keep'),false);assert(useEditor.getState().docs.keep);assert.equal(useEditor.getState().activeDocId,'keep');});});
test('switching projects removes obsolete tabs and loads the paired board',async()=>{const project:ProjectInfo={path:'A:/New/New.arproj',name:'New',sheets:[{file:'Schematic.arsch',docId:'new-sch'}],pcb:{file:'Board.arpcb',docId:'new-pcb'},libraries:[],variants:[]};await ipc((cmd,args)=>{if(cmd==='doc_snapshot')return snapshot(info(String(args?.id),{dirty:false,kind:args?.id==='new-pcb'?'pcb':'schematic'}));if(cmd==='lib_list')return [];throw Error(cmd);},async()=>{useEditor.getState().loadDoc(snapshot(info('old-sch')));useEditor.getState().loadDoc(snapshot(info('old-pcb')));useEditor.getState().setProject({...project,path:'A:/Old/Old.arproj',sheets:[{file:'old.arsch',docId:'old-sch'}],pcb:{file:'old.arpcb',docId:'old-pcb'}});await applyProjectState({project,messages:[]});assert(!useEditor.getState().docs['old-sch']);assert(!useEditor.getState().docs['old-pcb']);assert(useEditor.getState().docs['new-sch']);assert(useEditor.getState().docs['new-pcb']);});});
test('reviewed import applies only its token and chosen destination through IPC',async()=>{await ipc((cmd,args)=>{if(cmd==='import_apply'){assert.deepEqual(args,{token:'reviewed',dest:'A:/Work/Imported'});return {project:null,library,files:['A:/Work/Imported/Assets.arlib']};}throw Error(cmd);},async()=>{const result=await api.importApply('reviewed','A:/Work/Imported');assert.equal(result.library?.id,'library');});});
test('folder names cannot traverse paths or use reserved device names',()=>{for(const n of ['..','../Other','A/B','C:\\Other','CON','lpt1.txt','bad.',' bad','bad '])assert(folderNameError(n),n);for(const n of ['Project v1.2','Customer','Sensor-board'])assert.equal(folderNameError(n),null);});

test('saved-path refresh is emitted after a successful native Save without changing geometry',async()=>{const {bus}=await import('../src/core/bus.ts');const paths:string[][]=[];const stop=bus.on('filesSaved',e=>paths.push(e.paths));try{await ipc((cmd,args)=>{assert.equal(cmd,'doc_save');return info(String(args?.id),{dirty:false,path:'A:/Design/Board.arpcb'});},async()=>{useEditor.getState().loadDoc(snapshot(info('board',{kind:'pcb',path:'A:/Design/Board.arpcb'})));const items=useEditor.getState().docs.board.items;assert.equal(await saveDoc('board'),true);assert.deepEqual(paths,[['A:/Design/Board.arpcb']]);assert.equal(useEditor.getState().docs.board.items,items);});}finally{stop();}});


test('revision requests carry the reviewed project path through every native call',async()=>{const projectPath='A:/Design/Design.arproj',seen:string[]=[];await ipc((cmd,args)=>{assert.equal(args?.projectPath,projectPath);seen.push(cmd);return cmd==='project_revisions'?[]:cmd==='project_restore_copy'?'A:/Restored/Design.arproj':{};},async()=>{await api.projectRevisions(projectPath);await api.projectCheckpoint(projectPath,'Before edit');await api.projectCompareRevisions(projectPath,'r1','r2');await api.projectRestoreCopy(projectPath,'r1','A:/Restored');assert.deepEqual(seen,['project_revisions','project_checkpoint','project_compare_revisions','project_restore_copy']);});});

test('exported changelogs identify both checkpoints and retain details and omission limits',async()=>{
 const {revisionChangesText}=await import('../src/project/revisionText.ts');const revision={id:'r1',createdMs:0,label:'Before',files:2,bytes:10};
 const change={from:revision,to:{...revision,id:'r2',label:'After'},added:1,removed:1,modified:1,files:[{path:'Board.arpcb',status:'modified' as const,summary:'1 object modified',details:['footprint R1: pos changed'],omitted:3},{path:'Extra.arsch',status:'added' as const,summary:'New sheet',details:[],omitted:0},{path:'Old.arlib',status:'removed' as const,summary:'Library removed',details:[],omitted:0}]};
 const text=revisionChangesText(change);assert(text.includes('From: r1 · Before'));assert(text.includes('To: r2 · After'));assert(text.includes('1970-01-01T00:00:00.000Z'));assert(text.includes('1 files added · 1 removed · 1 modified'));assert(text.includes('footprint R1: pos changed'));assert(text.includes('3 additional detail lines omitted'));assert(text.includes('added · Extra.arsch'));assert(text.includes('removed · Old.arlib'));assert(text.endsWith('unsaved edits are excluded.\n'));assert(revisionChangesText({...change,files:[]}).includes('No saved file changes'));assert(revisionChangesText({...change,from:{...revision,createdMs:Number.MAX_VALUE}}).includes('Unknown date'));
});

test('automatic recovery never overlaps, respects disabling, retries errors and ignores disposed replies',async()=>{
 const {RecoveryTimer}=await import('../src/core/recoveryTimer.ts');let enabled=true,calls=0,finish:((v:unknown)=>void)|null=null;const successes:unknown[]=[],failures:unknown[]=[];
 const timer=new RecoveryTimer({enabled:()=>enabled,capture:()=>{calls++;return new Promise(resolve=>{finish=resolve;});},success:r=>successes.push(r),failure:e=>failures.push(e)});
 const first=timer.tick();await timer.tick();assert.equal(calls,1);finish!('first');await first;assert.deepEqual(successes,['first']);
 enabled=false;await timer.tick();assert.equal(calls,1);enabled=true;const late=timer.tick();timer.dispose();finish!('late');await late;assert.deepEqual(successes,['first']);assert.equal(failures.length,0);
 let attempts=0;const retry=new RecoveryTimer({enabled:()=>true,capture:async()=>{if(++attempts===1)throw Error('full');return 'recovered';},success:r=>successes.push(r),failure:e=>failures.push(e)});await retry.tick();await retry.tick();assert.equal(attempts,2);assert.equal(failures.length,1);assert.equal(successes.at(-1),'recovered');
});
test('close safety keeps the window open when unsaved-state checks or confirmation fail',async()=>{
 const {allowWindowClose}=await import('../src/core/closeSafety.ts');let copied=0,asked=0;const reports:string[]=[];
 const options={unsaved:async()=>['Board'],confirm:async()=>{asked++;return false;},capture:async()=>{copied++;},report:(m:string)=>reports.push(m)};
 assert.equal(await allowWindowClose({...options,unsaved:async()=>{throw Error('offline');}}),false);assert.equal(asked,0);
 assert.equal(await allowWindowClose(options),false);assert.equal(copied,0);
 assert.equal(await allowWindowClose({...options,confirm:async()=>{throw Error('dialog');}}),false);
 assert.equal(await allowWindowClose({...options,confirm:async()=>true}),true);assert.equal(copied,1);
 assert.equal(await allowWindowClose({...options,confirm:async()=>true,capture:async()=>{throw Error('full');}}),true);assert.equal(reports.length,3);
 assert.equal(await allowWindowClose({...options,unsaved:async()=>[]}),true);
});
test('Save Copy keeps the live document, project links, dirty state and Undo unchanged',async()=>{
 const {saveCopy}=await import('../src/core/actions.ts');const {bus}=await import('../src/core/bus.ts');
 const paths:string[][]=[];const off=bus.on('filesSaved',e=>paths.push(e.paths));
 try{await ipc((cmd,args)=>{
   if(cmd==='plugin:dialog|save'){assert.equal((args?.options as {defaultPath:string}).defaultPath,'A:/Design/Board-copy.arpcb');return 'A:/Copies/Board.arpcb';}
   if(cmd==='doc_save_copy'){assert.deepEqual(args,{id:'board',path:'A:/Copies/Board.arpcb'});return;}
   throw Error(cmd);
 },async()=>{
   useEditor.getState().loadDoc(snapshot(info('board',{kind:'pcb',path:'A:/Design/Board.arpcb',inProject:true})));
   const project:ProjectInfo={path:'A:/Design/Design.arproj',name:'Design',sheets:[],pcb:{file:'Board.arpcb',docId:'board'},libraries:[],variants:[]};
   useEditor.getState().setProject(project);const before=useEditor.getState().docs.board;
   assert.equal(await saveCopy('board'),true);assert.equal(useEditor.getState().docs.board,before);
   assert.equal(useEditor.getState().project,project);assert.deepEqual(paths,[['A:/Copies/Board.arpcb']]);
   assert(useEditor.getState().messages.some(m=>m.text.includes('working document is unchanged')));
 });}finally{off();}
});
test('Save Copy cancellation, native failure and a reloaded source cannot report success',async()=>{
 const {saveCopy}=await import('../src/core/actions.ts');
 for(const mode of ['cancel','fail','reload']){
  let copied=0;await ipc((cmd)=>{
   if(cmd==='plugin:dialog|save'){
    if(mode==='reload')useEditor.getState().loadDoc(snapshot(info('board')));
    return mode==='cancel'?null:'A:/Copy.arpcb';
   }
   if(cmd==='doc_save_copy'){copied++;throw Error('File already exists; choose a new filename');}
   throw Error(cmd);
  },async()=>{
   useEditor.getState().loadDoc(snapshot(info('board',{kind:'pcb'})));
   assert.equal(await saveCopy('board'),false);assert.equal(copied,mode==='fail'?1:0);
   assert(useEditor.getState().docs.board.info.dirty);
   assert(!useEditor.getState().messages.some(m=>m.text.startsWith('Saved a separate copy')));
  });
 }
});
test('external-save conflict preserves frontend geometry, identity and dirty state',async()=>{
 await ipc(cmd=>{assert.equal(cmd,'doc_save');throw Error('File changed outside PCBPlanner Studio. Use File > Save Copy.');},async()=>{
  useEditor.getState().loadDoc(snapshot(info('board',{kind:'pcb',path:'A:/Design/Board.arpcb'})));
  const before=useEditor.getState().docs.board;assert.equal(await saveDoc('board'),false);assert.equal(useEditor.getState().docs.board,before);
  assert(useEditor.getState().messages.some(m=>m.text.includes('Save Copy')));assert(!useEditor.getState().messages.some(m=>m.text.startsWith('Saved ')));
 });
});
test('Save All conflict refreshes native state without false completion or automatic overwrite',async()=>{
 const document=info('board',{kind:'pcb',path:'A:/Design/Board.arpcb'});
 await ipc(cmd=>{if(cmd==='project_save_all')throw Error('File changed outside PCBPlanner Studio');if(cmd==='doc_list')return [document];if(cmd==='lib_list')return [];throw Error(cmd);},async()=>{
  useEditor.getState().loadDoc(snapshot(document));const before=useEditor.getState().docs.board.items;
  assert.equal(await saveAll(),false);assert(useEditor.getState().docs.board.info.dirty);assert.equal(useEditor.getState().docs.board.items,before);
  assert(!useEditor.getState().messages.some(m=>m.text==='Saved all documents.'));
 });
});
test('Save Copy for a library-entry document requests a native library copy',async()=>{
 const {saveCopy}=await import('../src/core/actions.ts');
 await ipc((cmd,args)=>{
  if(cmd==='plugin:dialog|save'){assert.deepEqual((args?.options as {filters:unknown}).filters,[{name:'PCBPlanner Studio Library',extensions:['arlib']}]);return 'A:/Parts-copy.arlib';}
  if(cmd==='doc_save_copy'){assert.deepEqual(args,{id:'symbol',path:'A:/Parts-copy.arlib'});return;}
  throw Error(cmd);
 },async()=>{
  useEditor.getState().setLibs([{...library,path:'A:/Parts.arlib'}]);
  useEditor.getState().loadDoc(snapshot(info('symbol',{kind:'symbol',library:'library',entry:'Resistor'})));
  const before=useEditor.getState().libs;assert.equal(await saveCopy('symbol'),true);assert.equal(useEditor.getState().libs,before);assert(before[0].dirty);
 });
});
