import { useState } from 'react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Checkbox } from '@/components/ui/checkbox';
import { useEditor } from '../core/store';
import { formatGrid, GRID_PRESETS, parseLength, validGrid } from '../core/units';
import { Dialog, useModal } from './modal';
export function GridDialog({docId}:{docId:string}){
  const doc=useEditor(s=>s.docs[docId]);
  const units=useEditor(s=>s.units);
  const [text,setText]=useState(()=>formatGrid(doc?.grid??100000,units));
  const [snap,setSnap]=useState(doc?.view.snapToGrid??true),[visible,setVisible]=useState(doc?.view.grid??true);
  const value=parseLength(text,units),valid=value!==null&&validGrid(value);
  const close=()=>useModal.getState().close();
  const apply=()=>{if(!valid||value===null)return;useEditor.getState().setGrid(docId,value);useEditor.getState().setView(docId,{snapToGrid:snap,grid:visible});close();};
  return <Dialog title="Grid & snapping" description="Choose a precise grid in mm or mil. The step is remembered for this editor type." footer={<><Button variant="outline" onClick={close}>Cancel</Button><Button disabled={!valid||!doc} onClick={apply}>Apply</Button></>}>
    <label className="grid gap-1 text-xs">Grid step (bare numbers use {units})<Input autoFocus value={text} onChange={e=>setText(e.target.value)} onKeyDown={e=>{if(e.key==='Enter'&&valid){e.preventDefault();apply();}}}/></label>
    {!valid&&<p role="alert" className="mt-1 text-xs text-err-line">Enter a positive grid step up to 1000 mm, such as 0.125mm or 5mil.</p>}
    {(['mm','mil']as const).map(unit=><div className="mt-2 flex flex-wrap gap-1" key={unit}>{GRID_PRESETS[unit].map(step=><button key={step} className="rounded border border-line px-2 py-1 text-[11px] hover:bg-hover" aria-pressed={step===value} onClick={()=>setText(formatGrid(step,unit))}>{formatGrid(step,unit)}</button>)}</div>)}
    <label className="mt-3 flex items-center gap-2 text-xs"><Checkbox checked={snap} onCheckedChange={v=>setSnap(v===true)}/>Snap placement to the grid</label><label className="mt-2 flex items-center gap-2 text-xs"><Checkbox checked={visible} onCheckedChange={v=>setVisible(v===true)}/>Show grid dots</label>
    <p className="mt-2 text-xs text-muted-foreground">G / Shift+G steps through nearby presets. Object snapping to pads and endpoints remains available independently of grid snapping.</p>
  </Dialog>;
}
