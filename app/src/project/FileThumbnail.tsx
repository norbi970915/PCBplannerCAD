import {useEffect,useRef,useState,type ReactNode} from 'react';
import {api,type HubEntry} from '../core/api';
import {bus} from '../core/bus';
import {useTheme} from '../core/theme';
import {ThumbnailQueue,type Thumbnail} from './thumbnailQueue';
const queue=new ThumbnailQueue();
bus.on('filesSaved',()=>queue.clear());
export function clearThumbnailCache(){queue.clear();}
export function FileThumbnail({root,entry,version,fallback}:{root:string;entry:HubEntry;version:number;fallback:ReactNode}){
  const host=useRef<HTMLSpanElement>(null);const [visible,setVisible]=useState(false),[result,setResult]=useState<{key:string;image:Thumbnail}|null>(null);const ui=useTheme(s=>s.resolved);
  const supported=['project','schematic','board','library'].includes(entry.kind)||(entry.kind==='model'&&/\.(?:step|stp|wrl)$/i.test(entry.path));
  const key=JSON.stringify([root,entry.path,entry.modifiedMs,entry.bytes,version,queue.generation,ui]);
  useEffect(()=>{if(!supported||!host.current)return;const observer=new IntersectionObserver(entries=>setVisible(entries.some(e=>e.isIntersecting)),{rootMargin:'100px'});observer.observe(host.current);return()=>observer.disconnect();},[supported]);
  useEffect(()=>{
    if(!visible||!supported)return;
    return queue.request(key,async alive=>{const data=await api.hubPreview(root,entry.path);if(!alive())return null;const module=await import('./thumbnailRenderer');if(!alive())return null;const url=await module.rasterPreview(data,ui,alive);return {url,description:data.notes.join(' · ')||'Saved file content'};},image=>setResult({key,image}));
  },[key,visible,supported,root,entry.path,ui]);
  const image=result?.key===key?result.image:null;
  return <span ref={host} className="flex h-16 w-28 shrink-0 items-center justify-center overflow-hidden rounded border border-line bg-doc" title={image?.error||image?.description||(supported?'Preparing saved preview…':'File type')}>
    {image?.url?<img width={112} height={64} src={image.url} alt={'Saved preview of '+entry.name} className="h-full w-full object-contain"/>:fallback}
  </span>;
}
