import { useMemo, useState } from 'react';
import { Button } from '@/components/ui/button';
import { NativeSelect, NativeSelectOption } from '@/components/ui/native-select';
import { Input } from '@/components/ui/input';
import { Checkbox } from '@/components/ui/checkbox';
import { execute } from '../core/actions';
import { selectable } from '../core/selectFilter';
import { useEditor } from '../core/store';
import { Dialog, useModal } from '../shell/modal';
import { arrayAnchor, canArray, circularArray, rectangularArray, similarCriteria, similarItems, type SimilarOptions } from './selectionTools';

const close=()=>useModal.getState().close();
export function ArrayDialog({docId}:{docId:string}){
  const doc=useEditor(s=>s.docs[docId]);
  const [baseline]=useState(()=>doc?.items),[ids]=useState(()=>[...(doc?.selection??[])]);
  const [mode,setMode]=useState<'rectangular'|'circular'>('rectangular');
  const [fields,setFields]=useState(()=>{
    const anchor=doc?arrayAnchor(doc.items,doc.selection):{x:0,y:0};
    return {columns:'2',rows:'1',dx:'5',dy:'5',count:'6',angle:'60',cx:String(anchor.x/1e6-5),cy:String(anchor.y/1e6)};
  });
  const [even,setEven]=useState(true),[rotate,setRotate]=useState(true),[keepNets,setKeepNets]=useState(false),[busy,setBusy]=useState(false),[error,setError]=useState('');
  const source=ids.flatMap(id=>baseline?.get(id)?[baseline.get(id)!]:[]);
  const columns=Number(fields.columns),rows=Number(fields.rows),positions=Number(fields.count);
  const count=source.length*(mode==='rectangular'?columns*rows-1:positions-1);
  const valid=source.length>0&&source.every(canArray)&&Number.isInteger(count)&&count>0&&count<=10000;
  const apply=async()=>{
    if(busy)return;setError('');
    try{
      const current=useEditor.getState().docs[docId];
      if(!current||current.items!==baseline)throw Error('The board changed. Close and reopen the array dialog.');
      const required=mode==='rectangular'?['columns','rows',...(columns>1?['dx']:[]),...(rows>1?['dy']:[])]:['count','cx','cy',...(!even?['angle']:[])];
      if(required.some(key=>!fields[key as keyof typeof fields].trim()||!Number.isFinite(Number(fields[key as keyof typeof fields]))))throw Error('Complete the array fields with valid numbers.');
      const items=mode==='rectangular'
        ?rectangularArray(current.items,ids,{columns,rows,dx:Math.round(Number(fields.dx)*1e6),dy:Math.round(Number(fields.dy)*1e6),keepNets})
        :circularArray(current.items,ids,{count:positions,center:{x:Math.round(Number(fields.cx)*1e6),y:Math.round(Number(fields.cy)*1e6)},angle:even?360/positions:Number(fields.angle),rotate,keepNets});
      setBusy(true);if(await execute(docId,{op:'add',items}))close();else setError('The array could not be applied. The board was left unchanged.');
    }catch(e){setError(String(e));}finally{setBusy(false);}
  };
  const input=(key:keyof typeof fields,label:string)=><label key={key} className="grid gap-1 text-xs">{label}<Input type="number" step={key==='columns'||key==='rows'||key==='count'?1:0.1} value={fields[key]} disabled={busy} onChange={e=>setFields({...fields,[key]:e.target.value})}/></label>;
  return <Dialog title="Create array" description="Repeat the selected objects. The original stays in place; all copies are added in one undo step." footer={<><Button variant="outline" disabled={busy} onClick={close}>Cancel</Button><Button disabled={busy||!valid} onClick={()=>void apply()}>{busy?'Applying…':'Create array'}</Button></>}>
    <NativeSelect aria-label="Array pattern" disabled={busy} value={mode} onChange={e=>{setMode(e.target.value as typeof mode);setError('');}}><NativeSelectOption value="rectangular">Rectangular rows / columns</NativeSelectOption><NativeSelectOption value="circular">Circular pattern</NativeSelectOption></NativeSelect>
    <div className="mt-3 grid grid-cols-2 gap-3">{mode==='rectangular'?<>{input('columns','Columns (including original)')}{input('rows','Rows (including original)')}{input('dx','Column spacing X (mm)')}{input('dy','Row spacing Y (mm)')}</>:<>{input('count','Positions (including original)')}{!even&&input('angle','Step angle (degrees CCW)')}{input('cx','Center X (mm)')}{input('cy','Center Y (mm)')}</>}</div>
    {mode==='circular'&&<><label className="mt-3 flex items-center gap-2 text-xs"><Checkbox checked={even} disabled={busy} onCheckedChange={v=>setEven(v===true)}/>Even spacing around a full circle</label><label className="mt-2 flex items-center gap-2 text-xs"><Checkbox checked={rotate} disabled={busy} onCheckedChange={v=>setRotate(v===true)}/>Rotate each copy with the pattern</label></>}
    <label className="mt-3 flex items-center gap-2 text-xs"><Checkbox checked={keepNets} disabled={busy} onCheckedChange={v=>setKeepNets(v===true)}/>Keep the original nets on all copies</label>
    <p className="mt-2 text-xs text-muted-foreground">{keepNets?'Copied copper and pads share the existing nets.':'Copied copper and pads have no assigned net.'} Copies are unlocked. Component designators use unused numbers; no schematic channels are created. Positive Y goes upward. Copied pours need a refill.</p>
    <p className="mt-2 text-xs">{valid?count+' new objects':'Select supported board objects and a valid size (up to 10,000 new objects).'}</p>{error&&<p role="alert" className="mt-2 text-xs text-err-line">{error}</p>}
  </Dialog>;
}
export function SelectSimilarDialog({docId}:{docId:string}){
  const doc=useEditor(s=>s.docs[docId]);
  const [seed]=useState(()=>{const id=doc?.selection.values().next().value;return id===undefined?undefined:doc?.items.get(id);});
  const [options,setOptions]=useState<SimilarOptions>({layer:true,net:false,size:true,value:true,footprint:true});
  const [add,setAdd]=useState(false);
  const ids=useMemo(()=>doc&&seed?similarItems(doc.items,seed,options,item=>selectable(doc.info.kind,item,doc)):[],[doc,seed,options]);
  if(!seed)return <Dialog title="Select similar"><p>Select one object first.</p></Dialog>;
  const labels={layer:'Same layer / side / via span',net:'Same net',size:'Same width / size',value:'Same component value',footprint:'Same footprint'};
  const run=()=>{const current=useEditor.getState().docs[docId];if(!current)return;useEditor.getState().patchDoc(docId,{selection:new Set(add?[...current.selection,...similarItems(current.items,seed,options,item=>selectable(current.info.kind,item,current))]:similarItems(current.items,seed,options,item=>selectable(current.info.kind,item,current)))});useEditor.getState().setHint(ids.length+' matching objects selected');close();};
  return <Dialog title="Select similar" description={'Match objects of type '+seed.kind+'. Current visibility and selection filters are respected.'} footer={<><Button variant="outline" onClick={close}>Cancel</Button><Button disabled={!ids.length} onClick={run}>Select {ids.length}</Button></>}>
    <div className="grid gap-2">{similarCriteria(seed).map(key=><label key={key} className="flex items-center gap-2 text-xs"><Checkbox checked={options[key]} onCheckedChange={v=>setOptions({...options,[key]:v===true})}/>{labels[key]}</label>)}<label className="mt-2 flex items-center gap-2 text-xs"><Checkbox checked={add} onCheckedChange={v=>setAdd(v===true)}/>Add to current selection</label><p className="text-xs text-muted-foreground">{ids.length} matches. Clear all criteria to select every visible, selectable object of this type.</p></div>
  </Dialog>;
}
