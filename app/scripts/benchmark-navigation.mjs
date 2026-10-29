// Read-only CPU benchmark of real editor paths; does not measure GPU frame rate.
import fs from 'node:fs';
import { Scene } from '../src/editor/scene.ts';
import { HighlightTexture } from '../src/editor/highlightTexture.ts';
import { boardTheme } from '../src/editor/theme.ts';
import { DEFAULT_VIEW } from '../src/core/view.ts';
const input = process.argv[2];
if (!input) throw new Error('Usage: benchmark-navigation <native-board> [results.json]');
const data = JSON.parse(fs.readFileSync(input, 'utf8'));
const items = new Map(data.items.map(({ id, item }) => [id, item]));
const ctx = {kind:'pcb', cache:data.parts??{}, footprints:data.footprints??{}, view:DEFAULT_VIEW, pourFills:true, copperLayer:0};
const scene = new Scene();
let start = performance.now(); scene.update(items,ctx);
const initialMs = performance.now()-start, bounds=scene.bounds(), theme=boardTheme(0,'all');
const selection=new Set(), nets=new Map();
for(const [id,item] of items) if(item.net){let net=nets.get(item.net);if(!net)nets.set(item.net,net=[]);net.push(id);}
const marked=new Set([...nets.values()].sort((a,b)=>b.length-a.length)[0]??[]);
const layerChanges=[];
const board=[...items.values()].find(i=>i.kind==='board');
for(const layer of board?.stackup.layers.filter(l=>l.kind==='copper').map(l=>l.layer)??[]){const t=performance.now();scene.update(items,{...ctx,copperLayer:layer});layerChanges.push({layer,ms:+(performance.now()-t).toFixed(2)});}
scene.update(items,ctx);
const noop=()=>{};
let uploads=0;
const gl={TEXTURE0:0,TEXTURE_2D:1,TEXTURE_MIN_FILTER:2,TEXTURE_MAG_FILTER:3,NEAREST:4,TEXTURE_WRAP_S:5,TEXTURE_WRAP_T:6,CLAMP_TO_EDGE:7,RGBA8:8,RGBA:9,UNSIGNED_BYTE:10,MAX_TEXTURE_SIZE:11,createTexture:()=>({}),activeTexture:noop,bindTexture:noop,texParameteri:noop,getParameter:()=>16384,deleteTexture:noop,texImage2D:noop,texSubImage2D:()=>{uploads++;}};
const texture=new HighlightTexture(gl), pick=[], rect=[], hover=[], highlightedHover=[], edits=[];
const measure=(list,fn)=>{const t=performance.now();fn();list.push(performance.now()-t);};
const ids=[...items.keys()];
for(let i=0;i<140;i++){
 const p={x:bounds.minX+(bounds.maxX-bounds.minX)*((i*37%139)/139),y:bounds.minY+(bounds.maxY-bounds.minY)*((i*53%139)/139)};
 measure(pick,()=>scene.pick(p,100000));
 measure(rect,()=>scene.inRect({minX:p.x,minY:p.y,maxX:p.x+2000000,maxY:p.y+2000000},true));
 const hoverId=ids[i%ids.length];
 measure(hover,()=>scene.highlights(theme,{selection,hover:hoverId}));
 measure(highlightedHover,()=>texture.set(scene.highlights(theme,{selection,marked,hover:hoverId,dimOthers:true}),scene.styleCount));
}
for(let i=0;i<30;i++){
 const id=ids.find(id=>items.get(id).kind==='track');if(id===undefined)break;
 const old=items.get(id);const moved={...old,a:{x:old.a.x+1000,y:old.a.y},b:{x:old.b.x+1000,y:old.b.y}};
 items.set(id,moved);measure(edits,()=>{scene.update(items,ctx);scene.build(theme,{selection,hover:null});});items.set(id,old);
}
const summary=a=>{const sorted=a.slice(10).sort((a,b)=>a-b);const values=sorted.length?sorted:a.sort((a,b)=>a-b);return{medianMs:+values[Math.floor(values.length*.5)].toFixed(3),p95Ms:+values[Math.min(values.length-1,Math.floor(values.length*.95))].toFixed(3)};};
const result={layerChanges,objects:items.size,markedObjects:marked.size,initialMs:+initialMs.toFixed(2),pick:summary(pick),boxSelection:summary(rect),hover:summary(hover),netHighlightedHoverAndTintCPU:summary(highlightedHover),singleEditSceneCPU:edits.length?summary(edits):null,simulatedTextureUploads:uploads,note:'CPU work only; simulated texture calls exclude driver/GPU time. Not an FPS measurement.'};
console.log(JSON.stringify(result,null,2));if(process.argv[3])fs.writeFileSync(process.argv[3],JSON.stringify(result,null,2)+'\n');