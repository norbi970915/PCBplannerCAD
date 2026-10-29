import { CanvasTimings } from '../src/editor/canvasTimings.ts';
import test from 'node:test';
import assert from 'node:assert/strict';
import { InteractionFrame } from '../src/editor/interactionFrame.ts';
import { HighlightTexture } from '../src/editor/highlightTexture.ts';
import { Scene, highlightDimming } from '../src/editor/scene.ts';
import { drawItem, type DrawContext } from '../src/editor/draw.ts';
import { boardTheme } from '../src/editor/theme.ts';
import { DEFAULT_VIEW, cursorPoint, gridPoint, normalizeView } from '../src/core/view.ts';
import type { Item, Footprint } from '../src/core/types.ts';
const mm=(n:number)=>n*1e6, p=(x:number,y:number)=>({x:mm(x),y:mm(y)});
const track:Item={kind:'track',a:p(0,0),b:p(3,0),width:mm(.2),layer:0,net:'GND'};
const pad:Item={kind:'pad',pos:p(0,0),width:mm(2),height:mm(1),rotation:0,number:'1',shape:'rect',cornerRatio:0,padKind:'smd',drill:null,layer:0};
const text=(value:string,y:number):Item=>({kind:'text',text:value,pos:p(0,y),height:mm(.7),rotation:0,hAlign:'center',vAlign:'middle',mirrored:false,layer:2});
const fp:Footprint={name:'Test',description:'',items:[pad,text('.Designator',3),text('.Value',5),text('NOTE',7)],model:null};
const placed:Item={kind:'footprint',pos:p(0,0),rotation:0,side:'top',footprint:'Test',designator:'U1',value:'DEVICE',component:'',padNets:{'1':'GND'}};
const ctx=(patch={}):DrawContext=>({kind:'pcb',cache:{},footprints:{Test:fp},view:{...DEFAULT_VIEW,...patch}});
function frames(){const queue=new Map<number,FrameRequestCallback>();let seq=0,draws=0;const updates:number[]=[];
 const loop=new InteractionFrame<number>(v=>{updates.push(v);loop.invalidate();},()=>{draws++;},cb=>{queue.set(++seq,cb);return seq;},id=>{queue.delete(id);});
 return{loop,updates,get draws(){return draws;},get pending(){return queue.size;},tick(){const work=[...queue.values()];queue.clear();work.forEach(cb=>cb(0));}};}
test('500 pointer samples update and draw once at the latest position',()=>{const f=frames();for(let i=0;i<500;i++)f.loop.queue(i);assert.equal(f.pending,1);f.tick();assert.deepEqual(f.updates,[499]);assert.equal(f.draws,1);assert.equal(f.pending,0);});
test('pointer-up flushes the last drag endpoint and never replays it',()=>{const f=frames();f.loop.queue(10);f.loop.queue(42);f.loop.flush();assert.equal(f.updates.at(-1),42);f.tick();assert.deepEqual(f.updates,[42]);assert.equal(f.draws,1);});
test('unmount cancels pending input and rendering',()=>{const f=frames();f.loop.queue(1);f.loop.dispose();f.tick();f.loop.queue(2);f.loop.flush();assert.deepEqual(f.updates,[]);assert.equal(f.draws,0);assert.equal(f.pending,0);});
test('hover changes two texels without reallocating its table',()=>{let allocations=0;const updates:{x:number;y:number;bytes:number[]}[]=[];const noop=()=>{};
 const gl={TEXTURE0:0,TEXTURE_2D:1,TEXTURE_MIN_FILTER:2,TEXTURE_MAG_FILTER:3,NEAREST:4,TEXTURE_WRAP_S:5,TEXTURE_WRAP_T:6,CLAMP_TO_EDGE:7,RGBA8:8,RGBA:9,UNSIGNED_BYTE:10,MAX_TEXTURE_SIZE:11,createTexture:()=>({}),activeTexture:noop,bindTexture:noop,texParameteri:noop,getParameter:()=>4096,deleteTexture:noop,texImage2D:()=>{allocations++;},texSubImage2D:(_t:number,_l:number,x:number,y:number,_w:number,_h:number,_f:number,_k:number,b:Uint8Array)=>{updates.push({x,y,bytes:[...b]});}} as unknown as WebGL2RenderingContext;
 const tint=new HighlightTexture(gl);tint.set(new Map([[7,[1,1,1,.3]]]),6000);tint.set(new Map([[300,[1,1,1,.3]]]),6000);assert.equal(allocations,1);assert.equal(updates.length,2);assert.deepEqual(updates[0],{x:7,y:0,bytes:[0,0,0,0]});assert.deepEqual(updates[1],{x:44,y:1,bytes:[255,255,255,77]});tint.set(new Map([[300,[1,1,1,.3]]]),6000);assert.equal(updates.length,2);});
test('selection takes precedence over hover and retains cached primitives',()=>{const scene=new Scene();scene.update(new Map([[10,track]]),ctx());const before=scene.prims(10),theme=boardTheme(0,'all');const styles=scene.highlights(theme,{selection:new Set([10]),marked:new Set([10]),hover:10});assert.equal(styles.size,1);assert.equal([...styles.values()][0][3],.55);assert.equal(scene.prims(10),before);assert.equal(scene.highlights(theme,{selection:new Set(),hover:10,excluded:new Set([10])}).size,0);});
test('hidden layers disappear from drawing and picking and restore cleanly',()=>{const items=new Map([[1,track]]),scene=new Scene();scene.update(items,ctx());assert.equal(scene.pick(p(1,0),mm(.01)),1);scene.update(items,ctx({hiddenLayers:[0]}));assert.equal(scene.pick(p(1,0),mm(.01)),null);assert.equal(scene.prims(1)?.segs.length,0);scene.update(items,ctx());assert.equal(scene.pick(p(1,0),mm(.01)),1);});
test('pad numbers and net names can be hidden independently of copper',()=>{const normal=drawItem(placed,ctx()),noNumber=drawItem(placed,ctx({padNumbers:false}));assert(normal.segs.some(s=>s.role==='padNum'));assert(!noNumber.segs.some(s=>s.role==='padNum'));assert.equal(noNumber.fills.length,normal.fills.length);const netOnly=drawItem(placed,ctx({padNumbers:false,padNetNames:true}));assert(netOnly.segs.some(s=>s.role==='padNum'));assert.equal(netOnly.fills.length,normal.fills.length);});
test('reference, value and other text controls are independent',()=>{const normal=drawItem(placed,ctx());for(const flag of ['references','values','text']){const hidden=drawItem(placed,ctx({[flag]:false}));assert(hidden.segs.length<normal.segs.length);assert.equal(hidden.fills.length,normal.fills.length);}const hidden=drawItem(placed,ctx({references:false,values:false,text:false,padNumbers:false}));assert.equal(hidden.segs.length,0);assert(hidden.fills.length>0);});
test('bottom-side footprints follow flipped layer visibility',()=>{const bottom={...placed,side:'bottom'} as Item;const hidden=drawItem(bottom,ctx({hiddenLayers:[1,3]}));assert.equal(hidden.segs.length,0);assert.equal(hidden.fills.length,0);assert(drawItem(bottom,ctx({hiddenLayers:[0,2]})).fills.length>0);});
test('preference validation preserves explicit grid disable and filters malformed layers',()=>{const view=normalizeView({hiddenLayers:[0,0,33,'1',-1,100],snapToGrid:false,padNumbers:'false'});assert.deepEqual(view.hiddenLayers,[0,33]);assert.equal(view.snapToGrid,false);assert.equal(view.padNumbers,true);assert.deepEqual(gridPoint(p(.36,-.64),mm(1)),p(0,-1));assert.equal(normalizeView(null).snapToGrid,true);});

test('default browser frame callbacks keep their global receiver',()=>{
 const oldRequest=globalThis.requestAnimationFrame,oldCancel=globalThis.cancelAnimationFrame;let canceled=false;
 globalThis.requestAnimationFrame=function(){assert.equal(this,globalThis);return 1;};
 globalThis.cancelAnimationFrame=function(){assert.equal(this,globalThis);canceled=true;};
 try{const loop=new InteractionFrame<number>(()=>{},()=>{});loop.queue(1);loop.dispose();assert.equal(canceled,true);}
 finally{globalThis.requestAnimationFrame=oldRequest;globalThis.cancelAnimationFrame=oldCancel;}
});

test('free drag endpoints keep off-grid precision and produce integer native deltas',()=>{
 const start=cursorPoint({x:106556123.287312,y:-91204456.773},100000,false);
 const end=cursorPoint({x:109378814.4623,y:-93804111.291},100000,false);
 assert.equal(start.x,106556123);assert.equal(end.x,109378814);
 assert(Number.isSafeInteger(end.x-start.x));assert(Number.isSafeInteger(end.y-start.y));
 assert.notEqual((end.x-start.x)%100000,0);
 assert.deepEqual(cursorPoint({x:360000.1,y:-640000.9},1000000,true),p(0,-1));
});
test('toggling pour fills redraws only zones and reuses their triangulation',()=>{const outline=[p(0,0),p(10,0),p(10,10),p(0,10)];
 const zone:Item={kind:'zone',outline,fill:[{outer:outline,holes:[[p(2,2),p(4,2),p(4,4),p(2,4)]]}],layer:0,net:'GND',name:'',priority:0,clearance:0,minWidth:0,connect:'thermal',thermalGap:0,spokeWidth:0,keepout:false,removeIslands:false} as Item;
 const items=new Map<number,Item>([[1,track],[2,zone]]),scene=new Scene(),view={...DEFAULT_VIEW},cache={},footprints={};const at=(pourFills:boolean)=>({kind:'pcb' as const,cache,footprints,view,pourFills});
 scene.update(items,at(true));const trackPrims=scene.prims(1),tris=scene.prims(2)!.fills[0].tris;
 scene.update(items,at(false));assert.equal(scene.prims(1),trackPrims);assert.equal(scene.prims(2)!.fills.length,0);
 scene.update(items,at(true));assert.equal(scene.prims(1),trackPrims);assert.equal(scene.prims(2)!.fills[0].tris,tris);});
test('net highlight fades unmarked objects with a uniform and keeps hover/selection sparse',()=>{
 const scene=new Scene(),items=new Map<number,Item>();
 for(let id=1;id<=34000;id++)items.set(id,track);
 scene.update(items,ctx());const theme=boardTheme(0,'all');
 const state={selection:new Set([3]),hover:2,marked:new Set([1]),dimOthers:true};
 const styles=scene.highlights(theme,state);
 assert.equal(styles.size,3,'only marked, hovered and selected objects need texture entries');
 assert.deepEqual([...styles.values()].map(v=>v[3]),[.3,.4,.55]);
 assert.deepEqual(highlightDimming(theme,state),[...theme.background.slice(0,3),.75]);
 assert.deepEqual(highlightDimming(theme,{...state,marked:new Set()}),[0,0,0,0]);
 assert.deepEqual(highlightDimming(theme,{...state,dimOthers:false}),[0,0,0,0]);
});
test('index refits follow far moves, eventual rebalance, removals and hidden layers',()=>{
 const items=new Map<number,Item>();for(let id=0;id<80;id++)items.set(id,{...track,a:p(id*5,0),b:p(id*5+3,0)});
 const scene=new Scene(),view=ctx();scene.update(items,view);
 for(let step=0;step<300;step++){
  const id=step%80,x=500+(step%31)*7,y=10+(step%11)*5;
  const moved={...track,a:p(x,y),b:p(x+3,y)} as Item;items.set(id,moved);scene.update(items,view);
  const point=p(x+1,y);assert.notEqual(scene.pick(point,mm(.01)),null);
  const box={minX:point.x-mm(.1),minY:point.y-mm(.1),maxX:point.x+mm(.1),maxY:point.y+mm(.1)};
  const linear=[...items].filter(([,it])=>it.kind==='track'&&it.a.x<=box.maxX&&it.b.x>=box.minX&&it.a.y>=box.minY&&it.a.y<=box.maxY).map(([id])=>id).sort((a,b)=>a-b);
  assert.deepEqual(scene.inRect(box,true).sort((a,b)=>a-b),linear);
 }
 items.clear();scene.update(items,view);assert.deepEqual(scene.inRect({minX:-1e12,minY:-1e12,maxX:1e12,maxY:1e12},true),[]);
 items.set(1,track);scene.update(items,view);assert.equal(scene.pick(p(1,0),mm(.01)),1);
 scene.update(items,ctx({hiddenLayers:[0]}));assert.equal(scene.pick(p(1,0),mm(.01)),null);
 scene.update(items,view);assert.equal(scene.pick(p(1,0),mm(.01)),1);
});
test('local canvas timing recordings bound memory, report quantiles and reset cleanly',()=>{
 let now=100;const timings=new CanvasTimings(()=>now);assert.equal(timings.stop(),null);
 timings.start();for(let i=0;i<9000;i++)timings.frame(2);timings.frame(NaN);timings.frame(-1);
 now=110;assert.equal(timings.scene(()=>{now=115;return 'result';}),'result');
 const result=timings.stop()!;assert.equal(result.elapsedMs,15);assert.equal(result.frameCpu.count,9000);assert.equal(result.frameCpu.samples,8192);assert.equal(result.frameCpu.p95Ms,2);assert.equal(result.sceneCpu.medianMs,5);
 timings.frame(100);timings.start();assert.equal(timings.stop()!.frameCpu.count,0);
});

test('frame observers measure one coalesced frame and can be disabled',()=>{
 const f=frames(),samples:number[]=[];f.loop.setFrameObserver(ms=>samples.push(ms));
 f.loop.queue(1);f.loop.queue(2);f.tick();assert.equal(samples.length,1);assert(samples[0]>=0);
 f.loop.setFrameObserver(null);f.loop.queue(3);f.tick();assert.equal(samples.length,1);
});


test('render batches retain unchanged layers and pours while edits refresh only affected data',()=>{
 const outline=[p(0,0),p(10,0),p(10,10),p(0,10)];
 const zone:Item={kind:'zone',outline,fill:[{outer:outline,holes:[]}],layer:0,net:'GND',name:'',priority:0,clearance:0,minWidth:0,connect:'solid',thermalGap:0,spokeWidth:0,keepout:false,removeIslands:false};
 const items=new Map<number,Item>([[1,track],[2,{...track,layer:1}as Item],[3,zone]]),scene=new Scene(),context=ctx(),theme=boardTheme(0,'all'),hl={selection:new Set<number>(),hover:null};
 scene.update(items,context);const first=scene.build(theme,hl),snapshot=structuredClone(first);
 const second=scene.build(theme,hl);assert(first.every((g,i)=>g===second[i]),'unchanged batches are retained');
 const beforeTop=first.find(g=>g.rank===700)!,beforeOther=first.find(g=>g.rank!==700)!;
 items.set(1,{...track,a:p(0,2),b:p(3,2)}as Item);scene.update(items,context);
 const edited=scene.build(theme,hl),afterTop=edited.find(g=>g.rank===700)!;
 assert.notEqual(afterTop.lines,beforeTop.lines);assert.equal(afterTop.fills,beforeTop.fills,'unchanged pour triangles need no upload');
 assert(edited.includes(beforeOther),'other layer batch remains unchanged');assert.deepEqual(first,snapshot,'old submitted arrays remain immutable');
 const oracle=scene.build(theme,{selection:new Set([-1]),hover:null});assert.deepEqual(edited,oracle,'cached and independent assembly agree');
 for(const excluded of [new Set([1]),new Set([2,3]),new Set<number>()]){
  assert.deepEqual(scene.build(theme,{...hl,excluded}),scene.build(theme,{selection:new Set([-1]),hover:null,excluded}));
 }
 const changedTheme=boardTheme(1,'only');assert.deepEqual(scene.build(changedTheme,hl),scene.build(changedTheme,{selection:new Set([-1]),hover:null}));
 items.delete(3);scene.update(items,context);assert.deepEqual(scene.build(theme,hl),scene.build(theme,{selection:new Set([-1]),hover:null}));
 items.clear();scene.update(items,context);assert.deepEqual(scene.build(theme,hl),[]);
});

test('cached rendering follows hidden layers, pour toggles and restored geometry',()=>{
 const outline=[p(0,0),p(5,0),p(5,5),p(0,5)];
 const zone:Item={kind:'zone',outline,fill:[{outer:outline,holes:[]}],layer:0,net:'GND',name:'',priority:0,clearance:0,minWidth:0,connect:'solid',thermalGap:0,spokeWidth:0,keepout:false,removeIslands:false};
 const items=new Map<number,Item>([[1,track],[2,zone]]),scene=new Scene(),context=ctx(),theme=boardTheme(0,'all'),hl={selection:new Set<number>(),hover:null};
 scene.update(items,context);const original=structuredClone(scene.build(theme,hl));
 scene.update(items,{...context,pourFills:false});assert.equal(scene.build(theme,hl).flatMap(g=>g.fills).length,0);
 scene.update(items,{...context,pourFills:true});assert.deepEqual(scene.build(theme,hl),original);
 scene.update(items,ctx({hiddenLayers:[0]}));assert.deepEqual(scene.build(theme,hl),[]);
 scene.update(items,context);assert.deepEqual(scene.build(theme,hl),original);
});


test('saved display views validate local data, bound names/count and preserve interaction settings',async()=>{
 const {displaySettings,normalizeViewPresets,saveViewPreset,readViewPresets,viewPresetsKey}=await import('../src/core/viewPresets.ts');
 const snapshot={name:'Routing',display:displaySettings({...DEFAULT_VIEW,hiddenLayers:[1,1,-3,Infinity],values:false,grid:false,snapToGrid:false}),layerMode:'dim' as const,pourFills:false};
 assert.deepEqual(snapshot.display.hiddenLayers,[1]);assert.equal('grid' in snapshot.display,false);assert.equal('snapToGrid' in snapshot.display,false);
 const first=saveViewPreset([],snapshot);assert.throws(()=>saveViewPreset(first,{...snapshot,name:' routing '}),/already exists/);assert.equal(first[0].display.values,false);
 const replaced=saveViewPreset(first,{...snapshot,name:'Routing',display:displaySettings(DEFAULT_VIEW)},true);assert.equal(replaced.length,1);assert.equal(replaced[0].display.values,true);assert.equal(first[0].display.values,false);
 assert.throws(()=>saveViewPreset([], {...snapshot,name:' '}),/name/);assert.throws(()=>saveViewPreset([], {...snapshot,name:'x'.repeat(61)}),/name/);
 const list=normalizeViewPresets([null,{},snapshot,{...snapshot,name:'routing'},...Array.from({length:40},(_,i)=>({...snapshot,name:'View '+i}))]);assert.equal(list.length,24);assert.equal(list.filter(p=>p.name.toLowerCase()==='routing').length,1);assert.throws(()=>saveViewPreset(list,{...snapshot,name:'Extra'}),/limit 24/);
 assert.deepEqual(readViewPresets('pcb',{getItem:()=>'{broken'}),[]);assert.deepEqual(readViewPresets('pcb',{getItem:()=>{throw Error('unavailable');}}),[]);
 assert.deepEqual(readViewPresets('pcb',{getItem:()=>JSON.stringify([snapshot])}),first);assert.notEqual(viewPresetsKey('pcb'),viewPresetsKey('schematic'));
 const current=normalizeView({...DEFAULT_VIEW,grid:false,snapToGrid:false});const applied=normalizeView({...current,...snapshot.display});assert.equal(applied.grid,false);assert.equal(applied.snapToGrid,false);assert.equal(applied.values,false);
});
