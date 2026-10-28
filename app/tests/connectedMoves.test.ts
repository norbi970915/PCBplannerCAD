import test from 'node:test';
import assert from 'node:assert/strict';
import { mockIPC, clearMocks } from '@tauri-apps/api/mocks';
import type { DocState } from '../src/core/store.ts';
import type { Item, PcbDragPlan } from '../src/core/types.ts';
const prefs=new Map<string,string>();Object.defineProperty(globalThis,'localStorage',{configurable:true,value:{getItem:(key:string)=>prefs.get(key)??null,setItem:(key:string,value:string)=>prefs.set(key,value),removeItem:(key:string)=>prefs.delete(key)}});
const {useEditor}=await import('../src/core/store.ts');
const {align,packAndMove}=await import('../src/pcb/boardActions.ts');
const {bus}=await import('../src/core/bus.ts');
const {moveRejection}=await import('../src/editor/connectedMove.ts');
Object.defineProperty(globalThis,'window',{configurable:true,value:{setTimeout:()=>1,clearTimeout:()=>{}}});
const p=(x:number,y:number)=>({x:x*1e6,y:y*1e6});let next=0;
function setup(planner?:(request:any)=>PcbDragPlan|Promise<PcbDragPlan>){
 const id='connected-'+next++,calls:{command:string;args:any}[]=[];
 const footprint=(designator:string,x:number,y:number):Item=>({kind:'footprint',designator,value:'',component:'',parameters:{},footprint:'F',pos:p(x,y),rotation:0,side:'top',padNets:{'1':'A'},locked:false});
 const items=new Map<number,Item>([[1,footprint('U1',0,0)],[2,footprint('U2',3,4)],[3,footprint('U3',20,-3)],[100,{kind:'track',a:p(0,0),b:p(3,4),width:200000,layer:0,net:'A'}]]);
 const doc={info:{id,kind:'pcb',title:'Test'},items,footprints:{F:{name:'F',description:'',model:null,items:[{kind:'pad',pos:p(0,0),width:1e6,height:1e6,rotation:0,number:'1',shape:'rect',cornerRatio:0,padKind:'smd',drill:null,layer:0}]}},cache:{},selection:new Set([1,2,3]),epoch:1,grid:1e6,tool:'select'}as unknown as DocState;
 useEditor.setState({docs:{[id]:doc},activeDocId:id,drcEnforce:true});useEditor.getState().setPlacement({moveMode:'preserveAngles',moveViolators:'walkaround',cornerStyle:'diagonal45'});
 const plan=(request:any):PcbDragPlan=>({replace:[...request.moves.map((move:any)=>{const item=items.get(move.id)as Extract<Item,{kind:'footprint'}>;return{id:move.id,item:{...item,pos:{x:item.pos.x+move.delta.x,y:item.pos.y+move.delta.y}}};}),{id:100,item:items.get(100)!}],delete:[],add:[],rerouted:1,blocked:0});
 mockIPC((command,args)=>{calls.push({command,args});if(command==='pcb_drag_each')return planner?planner(args!.req):plan(args!.req);if(command==='doc_execute'){const cmd=args!.cmd;return{info:doc.info,delta:{upserted:cmd.op==='route'?cmd.replace:cmd.items,removed:cmd.delete??[]}};}throw Error('Unexpected command '+command);});
 return{id,doc,items,calls,plan,commands:()=>calls.filter(c=>c.command==='doc_execute'),close:()=>clearMocks()};
}

test('alignment and distribution commit through one connected routing transaction',async()=>{
 for(const mode of ['centerY','distributeX']as const){const h=setup();try{
  await align(mode);const planning=h.calls.find(c=>c.command==='pcb_drag_each');assert(planning);assert.equal(planning.args.req.mode,'preserveAngles');assert.equal(planning.args.req.routeMode,'walkaround');
  assert.equal(h.commands().length,1);assert.equal(h.commands()[0].args.cmd.op,'route');assert(h.commands()[0].args.cmd.replace.some((item:any)=>item.id===100),'connected copper comes from the native plan');
 }finally{h.close();}}
});

test('explicit Disconnect keeps direct placement behavior for alignment',async()=>{
 const h=setup();try{useEditor.getState().setPlacement({moveMode:'disconnect'});await align('top');assert(!h.calls.some(c=>c.command==='pcb_drag_each'));assert.equal(h.commands().length,1);assert.equal(h.commands()[0].args.cmd.op,'replaceMany');}finally{h.close();}
});

test('failed Pack & Move does not commit partial edits or pick up the selection',async()=>{
 const h=setup(()=>({replace:[],add:[],delete:[],rerouted:0,blocked:1,failure:'locked'}));const picked:unknown[]=[];const off=bus.on('pickUp',event=>picked.push(event));
 try{await packAndMove();assert.equal(h.commands().length,0);assert.deepEqual(picked,[]);assert.equal(useEditor.getState().docs[h.id].items,h.items);assert.match(useEditor.getState().hint,/locked/);}finally{off();h.close();}
});

test('pending placement replies reject changed board, library, selection, settings or document',async()=>{
 for(const change of ['items','footprints','selection','epoch','settings','active']as const){let settle!:(plan:PcbDragPlan)=>void;const h=setup(()=>new Promise(resolve=>settle=resolve));
  try{const operation=align('left');const request=h.calls[0].args.req;
   if(change==='items')useEditor.getState().patchDoc(h.id,{items:new Map(h.items)});
   if(change==='footprints')useEditor.getState().patchDoc(h.id,{footprints:{...h.doc.footprints}});
   if(change==='selection')useEditor.getState().patchDoc(h.id,{selection:new Set([1,2])});
   if(change==='epoch')useEditor.getState().patchDoc(h.id,{epoch:2});
   if(change==='settings')useEditor.setState({drcEnforce:false});
   if(change==='active')useEditor.setState({activeDocId:'other'});
   settle(h.plan(request));await operation;assert.equal(h.commands().length,0,change+' must invalidate planning');
  }finally{h.close();}
 }
});

test('only the newest overlapping alignment plan commits',async()=>{
 const settle:((plan:PcbDragPlan)=>void)[]=[];const h=setup(()=>new Promise(resolve=>settle.push(resolve)));
 try{const first=align('centerY'),second=align('left');settle[0](h.plan(h.calls[0].args.req));await first;assert.equal(h.commands().length,0);
  settle[1](h.plan(h.calls[1].args.req));await second;assert.equal(h.commands().length,1);assert.equal(h.commands()[0].args.cmd.op,'route');
 }finally{h.close();}
});

test('failure reasons reject even a malformed plan containing edits',()=>{
 const plan={replace:[{id:1,item:{kind:'via'}}],add:[],delete:[],rerouted:1,blocked:1,failure:'unroutable'}as unknown as PcbDragPlan;
 assert.match(moveRejection(plan)!,/No clear route/);assert.equal(moveRejection({...plan,failure:undefined,blocked:0}),null);
});


test('a legacy incomplete plan cannot bypass avoid mode but explicit Ignore remains available',()=>{
 const plan={replace:[{id:1,item:{kind:'via'}}],add:[],delete:[],rerouted:0,blocked:1}as unknown as PcbDragPlan;
 assert.match(moveRejection(plan,'walkaround')!,/No clear route/);assert.match(moveRejection(plan,'shove')!,/No clear route/);assert.equal(moveRejection(plan,'ignore'),null);
});
