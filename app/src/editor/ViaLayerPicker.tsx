import { useEffect,useRef,useState } from 'react';
export interface ViaLayerRequest {layers:{id:number;name:string;color:string}[];current:number;suggested:number;x:number;y:number}
export function ViaLayerPicker({request,onChoose}:{request:ViaLayerRequest;onChoose:(layer:number|null)=>void}){
  const layers=request.layers.filter(layer=>layer.id!==request.current);
  const [selected,setSelected]=useState(()=>Math.max(0,layers.findIndex(layer=>layer.id===request.suggested)));
  const buttons=useRef<(HTMLButtonElement|null)[]>([]);
  useEffect(()=>{buttons.current[selected]?.focus();},[selected]);
  useEffect(()=>{
    const key=(e:KeyboardEvent)=>{
      e.preventDefault();e.stopImmediatePropagation();
      if(e.key==='Escape')onChoose(null);
      else if(e.key==='ArrowDown'||e.key==='ArrowUp'||e.key==='Tab')setSelected(index=>(index+((e.key==='ArrowUp'||e.key==='Tab'&&e.shiftKey)?-1:1)+layers.length)%layers.length);
      else if(e.key==='Enter'&&!e.repeat)onChoose(layers[selected]?.id??null);
    };
    window.addEventListener('keydown',key,true);return()=>window.removeEventListener('keydown',key,true);
  },[layers.length,selected,onChoose]);
  return <div className="absolute inset-0 z-50" onPointerDown={e=>{e.stopPropagation();onChoose(null);}} onContextMenu={e=>{e.preventDefault();e.stopPropagation();onChoose(null);}}>
    <div role="dialog" aria-modal="true" aria-label="Continue routing on layer" className="absolute w-52 rounded border border-line-strong bg-card p-1.5 text-xs shadow-xl" style={{left:request.x,top:request.y}} onPointerDown={e=>e.stopPropagation()}>
      <div className="px-2 py-1 font-semibold">Via → continue on layer</div>
      <div className="px-2 pb-1 text-[10px] text-faint">From {request.layers.find(layer=>layer.id===request.current)?.name}</div>
      <div className="max-h-52 overflow-auto">{layers.map((layer,index)=><button ref={node=>{buttons.current[index]=node;}} key={layer.id} className={'flex w-full items-center gap-2 rounded px-2 py-1.5 text-left outline-none '+(selected===index?'bg-sel text-accent-ink':'hover:bg-hover')} onFocus={()=>setSelected(index)} onClick={()=>onChoose(layer.id)}><span className="size-2.5 rounded-sm" style={{background:layer.color}}/>{layer.name}</button>)}</div>
      <div className="px-2 pt-1 text-[10px] text-faint">↑↓ choose · Enter confirm · Esc cancel</div>
    </div>
  </div>;
}
