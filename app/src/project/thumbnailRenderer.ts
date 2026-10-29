import type {SavedPreview} from '../core/api';
import {Renderer} from '../editor/renderer';
import {Scene} from '../editor/scene';
import {Camera} from '../editor/camera';
import {themeFor} from '../editor/theme';
let canvas:HTMLCanvasElement|null=null,renderer:Renderer|null=null;
/** One shared drawing context paints all 2D thumbnails; cards only retain PNGs. */
export async function rasterPreview(preview:SavedPreview,ui:'light'|'dark',alive:()=>boolean):Promise<string|null>{
  if(preview.model){const module=await import('./thumbnailModel');return alive()?module.modelThumbnail(preview.model,ui,alive):null;}
  if(!preview.views.length)return null;
  canvas??=document.createElement('canvas');renderer??=new Renderer(canvas);
  const output=document.createElement('canvas');output.width=224;output.height=128;const ctx=output.getContext('2d');if(!ctx)return null;
  ctx.fillStyle=ui==='dark'?'#1b1f28':'#f1f4f8';ctx.fillRect(0,0,output.width,output.height);
  const views=preview.views.slice(0,2),width=Math.floor(output.width/views.length);
  for(let index=0;index<views.length;index++){
    const v=views[index],scene=new Scene(),cam=new Camera();cam.width=width;cam.height=108;renderer.resize(width,108,1);
    const theme=themeFor(v.kind,0,'all',ui);
    scene.update(new Map(v.items.map((item,id)=>[id,item])),{kind:v.kind,cache:v.cache,footprints:v.footprints,symbol:v.symbol,pourFills:false});
    renderer.setScene(scene.build(theme,{selection:new Set(),hover:null}));renderer.setHighlights(new Map(),scene.styleCount);renderer.setOverlay([]);
    const bounds=scene.bounds();if(bounds)cam.fit(bounds,7);renderer.render(cam,{step:1e12,background:theme.background,...theme.grid});
    ctx.drawImage(canvas,index*width,20,width,108);ctx.font='11px sans-serif';ctx.fillStyle=ui==='dark'?'#d0d6df':'#354052';ctx.fillText(v.label,index*width+5,14);
    if(!bounds){ctx.fillStyle=ui==='dark'?'#a0a8b4':'#687487';ctx.fillText('Empty',index*width+Math.max(5,width/2-16),75);}
  }
  return alive()?output.toDataURL('image/png'):null;
}
