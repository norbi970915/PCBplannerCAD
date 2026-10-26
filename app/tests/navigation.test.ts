import test from 'node:test';
import assert from 'node:assert/strict';
import { InteractionFrame } from '../src/editor/interactionFrame.ts';
import { HighlightTexture } from '../src/editor/highlightTexture.ts';
import { Scene } from '../src/editor/scene.ts';
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
test('selection takes precedence over hover and retains cached primitives',()=>{const scene=new Scene();scene.update(new Map([[10,track]]),ctx());const before=scene.prims(10),theme=boardTheme(0,false);const styles=scene.highlights(theme,{selection:new Set([10]),marked:new Set([10]),hover:10});assert.equal(styles.size,1);assert.equal([...styles.values()][0][3],.55);assert.equal(scene.prims(10),before);assert.equal(scene.highlights(theme,{selection:new Set(),hover:10,excluded:new Set([10])}).size,0);});
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
