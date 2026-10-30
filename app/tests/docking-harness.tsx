// Isolated shared-window fixture: synthetic geometry and mocked native IPC only.
import {createRoot} from 'react-dom/client';
import {emit} from '@tauri-apps/api/event';
import {mockIPC,mockWindows} from '@tauri-apps/api/mocks';
import '../src/styles/globals.css';
import 'dockview-react/dist/styles/dockview.css';
import '../src/styles/app.css';
import {App} from '../src/App';
import {TooltipProvider} from '../src/components/ui/tooltip';
import {useEditor} from '../src/core/store';
import {useTheme} from '../src/core/theme';
import {useRecoveryStatus} from '../src/core/recovery';
import {api} from '../src/core/api';
import {runDrc} from '../src/core/pcbActions';
import {bus} from '../src/core/bus';
import {revealLinkedComponent} from '../src/core/crossProbe';
import {useModal,Dialog} from '../src/shell/modal';
import type {Snapshot,Item,LibSymbol,Component} from '../src/core/types';
mockWindows('main');
const calls:{command:string;args:unknown}[]=[];
const picks:(string|null)[]=[];let copyError=false;
mockIPC((command,args)=>{
 calls.push({command,args});
 if(command==='plugin:dialog|save')return picks.shift()??null;
 if(command==='doc_save_copy'){if(copyError){copyError=false;throw Error('File already exists; choose a new filename.');}return;}
 if(command==='doc_save')throw Error('File changed outside PCBPlanner Studio. Your edits are still open; use File > Save Copy.');
 if(command==='recovery_list')return {location:'isolated',entries:[],issues:[]};
 if(command==='lib_list'||command==='doc_list'||command==='lib_component_index')return [];
 if(command==='pcb_object_at')return null;
 if(command==='plugin:event|listen')return 1;
 if(command==='plugin:event|unlisten'||command.startsWith('plugin:window|')||command==='dock_close_window'||command==='dock_focus_window')return;
 if(command==='doc_undo'||command==='doc_redo'){
  const info=useEditor.getState().docs[args!.id as string].info;
  return {info:{...info,canUndo:command==='doc_redo',canRedo:command==='doc_undo'},delta:{removed:[],upserted:[]},touchedLibrary:null};
 }
 if(command==='pcb_drc')return {stats:{nets:1,connections:1,unrouted:0},violations:[{severity:'error',code:'clearance',message:'Detached board finding',items:[1],pos:{x:30e6,y:20e6},layer:0,rule:'General clearance'}],ruleErrors:[]};
 if(command==='pcb_ratsnest')return [];
 if(command==='pcb_route')return {tracks:[],reached:false,moved:[],blockedAt:null,message:''};
 throw Error('Unexpected test IPC '+command);
},{shouldMockEvents:true});
api.docList=async()=>Object.values(useEditor.getState().docs).map(d=>d.info);
useRecoveryStatus.setState({enabled:false});
useEditor.setState({onlineDrc:false});
const field={name:'Designator',value:'R1',offset:{x:0,y:-3e6},height:1e6,rotation:0,hAlign:'left' as const,vAlign:'bottom' as const,visible:true};
const body:Item={kind:'rect',a:{x:-2e6,y:-1e6},b:{x:2e6,y:1e6},width:200000,layer:100,fill:'none'};
const symbol:LibSymbol={name:'Resistor',description:'',prefix:'R',units:[{name:'',items:[body]}],showPinNumbers:true,showPinNames:true,pinNameOffset:0,isPower:false,fields:[]};
const component:Component={name:'Resistor',description:'',symbol:'Resistor',footprints:['Resistor'],pinMap:[],parameters:[]};
const part:Item={kind:'part',pos:{x:0,y:0},rotation:0,mirror:false,libRef:'resistor',unit:0,fields:[field]};
const footprint:Item={kind:'footprint',designator:'R1',value:'10k',footprint:'Resistor',pos:{x:30e6,y:20e6},rotation:0,side:'top',locked:false,padNets:{},component:'resistor',parameters:{}};
const pcbBody:Item={...body,layer:2};
const pad:Item={kind:'pad',number:'1',pos:{x:-2e6,y:0},rotation:0,shape:'rect',width:1e6,height:2e6,cornerRatio:0,padKind:'smd',layer:0,drill:null};
const load=(id:string,kind:'pcb'|'schematic',items:Item[])=>{
 const snapshot:Snapshot={info:{id,kind,title:kind==='pcb'?'Board':'Schematic',path:null,dirty:true,canUndo:true,canRedo:false,library:null,entry:null,unit:null,inProject:true},items:items.map((item,i)=>({id:i+1,item})),cache:{resistor:{library:'Fixture',component,symbol}},footprints:{Resistor:{name:'Resistor',description:'',items:[pcbBody,pad,{...pad,number:'2',pos:{x:2e6,y:0}}],model:null}}};
 useEditor.getState().loadDoc(snapshot);
};
load('sch','schematic',[part]);load('pcb','pcb',[footprint,{kind:'track',a:{x:0,y:0},b:{x:100e6,y:50e6},width:200000,layer:0,net:''}]);
useEditor.getState().setProject({path:'A:/Fixture/Fixture.arproj',name:'Fixture',sheets:[{file:'Schematic.arsch',docId:'sch'}],pcb:{file:'Board.arpcb',docId:'pcb'},libraries:[],variants:[]});
useTheme.getState().setTheme('dark');
createRoot(document.getElementById('root')!).render(<TooltipProvider><App/></TooltipProvider>);
Object.assign(window,{dockTest:{
 calls,picks,runDrc:()=>runDrc('pcb'),badCopy:()=>{copyError=true;},
 nativeClose:(label:string)=>emit("pcad-dock-close",label),
 open:(docId:string)=>bus.emit('showDoc',{docId}),
 state:()=>{const s=useEditor.getState();return {path:s.docs.pcb.info.path,linked:s.project?.pcb?.docId,hint:s.hint,messages:s.messages.map(m=>m.text),active:s.activeDocId,tool:s.docs.pcb.tool,selection:[...s.docs.pcb.selection],dirty:s.docs.pcb.info.dirty,undo:s.docs.pcb.info.canUndo,items:JSON.stringify([...s.docs.pcb.items]),focus:s.docs.pcb.focus};},
 reveal:()=>revealLinkedComponent('sch',1),
 toggle:(id:string)=>bus.emit('togglePanelWindow',{id}),
 theme:(value:'light'|'dark')=>useTheme.getState().setTheme(value),
 modal:()=>useModal.getState().show(<Dialog title="Detached action"><p>Shared project dialog</p></Dialog>),
 reset:()=>bus.emit('resetLayout',{}),
}});
