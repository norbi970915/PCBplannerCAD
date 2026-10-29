import test from 'node:test';
import assert from 'node:assert/strict';
import {mockIPC,clearMocks} from '@tauri-apps/api/mocks';
import {rotateWithConnections} from '../src/editor/connectedRotation.ts';
import {useEditor,type DocState} from '../src/core/store.ts';
import type {Command,Item,PcbDragPlan} from '../src/core/types.ts';
const center={x:10e6,y:10e6};
const fp:Item={kind:'footprint',designator:'R1',value:'10k',footprint:'F',component:'',parameters:{},side:'top',pos:center,rotation:0,padNets:{'2':'SIG'},locked:false};
const wire:Item={kind:'track',a:{x:12e6,y:10e6},b:{x:32e6,y:10e6},net:'SIG',layer:0,width:250000};
const plan:PcbDragPlan={replace:[{id:1,item:{...fp,rotation:90}},{id:2,item:{...wire,a:{x:10e6,y:12e6}}}],add:[],delete:[],rerouted:1,blocked:0};
const document=()=>({info:{id:'rotation',kind:'pcb'},items:new Map([[1,fp],[2,wire]]),footprints:{},selection:new Set([1]),epoch:1}) as unknown as DocState;
async function setup(run:(h:{doc:()=>DocState;setDoc:(d:DocState)=>void;commands:Command[];rotate:(live?:()=>boolean)=>Promise<boolean>})=>Promise<void>,handler:Parameters<typeof mockIPC>[0]=()=>plan){
 const old=useEditor.getState();const oldWindow=Object.getOwnPropertyDescriptor(globalThis,'window');Object.defineProperty(globalThis,'window',{configurable:true,value:{}});mockIPC(handler);let doc=document();const commands:Command[]=[];
 useEditor.setState({placement:{...old.placement,moveMode:'preserveAngles',moveViolators:'walkaround',cornerStyle:'diagonal45'},drcEnforce:true});
 try{await run({doc:()=>doc,setDoc:d=>{doc=d;},commands,rotate:live=>rotateWithConnections('rotation',[1],center,90,async cmd=>{commands.push(cmd);return true;},()=>doc,live)});}finally{clearMocks();useEditor.setState({placement:old.placement,drcEnforce:old.drcEnforce});if(oldWindow)Object.defineProperty(globalThis,'window',oldWindow);else Reflect.deleteProperty(globalThis,'window');}
}
test('idle rotation commits footprint and attached route together, retaining IDs',async()=>{await setup(async h=>{assert(await h.rotate());assert.deepEqual(h.commands,[{op:'route',delete:[],add:[],replace:plan.replace}]);},(cmd,args)=>{assert.equal(cmd,'pcb_drag_footprints');const req=args!.req as any;assert.equal(req.angle,90);assert.deepEqual(req.center,center);assert.deepEqual(req.delta,{x:0,y:0});return plan;});});
test('blocked rotation applies no geometry or history',async()=>{await setup(async h=>{await assert.rejects(h.rotate(),/No clear route/);assert.deepEqual(h.commands,[]);},()=>({...plan,blocked:1,failure:'unroutable'}));});
test('rotation ignores stale geometry, definitions, selection, epoch or settings',async()=>{for(const change of ['items','footprints','selection','epoch','settings']){let settle!:(p:PcbDragPlan)=>void;await setup(async h=>{const pending=h.rotate();const d=h.doc();if(change==='settings')useEditor.getState().setPlacement({moveMode:'disconnect'});else h.setDoc({...d,...(change==='items'?{items:new Map(d.items)}:change==='footprints'?{footprints:{}}:change==='selection'?{selection:new Set()}: {epoch:d.epoch+1})});settle(plan);await assert.rejects(pending,/changed while planning/);assert.deepEqual(h.commands,[]);},()=>new Promise(resolve=>{settle=resolve;}));}});
test('cancelled rotation cannot commit a late plan; Disconnect stays explicit',async()=>{let settle!:(p:PcbDragPlan)=>void;await setup(async h=>{let current=true;const pending=h.rotate(()=>current);current=false;settle(plan);assert.equal(await pending,false);assert.deepEqual(h.commands,[]);useEditor.getState().setPlacement({moveMode:'disconnect'});assert(await h.rotate());assert.deepEqual(h.commands,[{op:'rotate',ids:[1],center,angle:90}]);},()=>new Promise(resolve=>{settle=resolve;}));});
