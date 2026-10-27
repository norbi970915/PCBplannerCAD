import { useState } from 'react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { NativeSelect, NativeSelectOption } from '@/components/ui/native-select';
import { Checkbox } from '@/components/ui/checkbox';
import { Dialog, useModal } from '../shell/modal';
import { execute } from '../core/actions';
import { useEditor } from '../core/store';
import { boardCopper, layer } from '../core/layers';
import type { ItemId, ZoneItem } from '../core/types';
import { pourOrder, reorderPours } from './editing';

export function PourManager({docId}:{docId:string}) {
  const doc=useEditor(s=>s.docs[docId]);
  const [baseline]=useState(doc.items);
  const [draft,setDraft]=useState(()=>new Map(doc.items));
  const [selected,setSelected]=useState<ItemId[]>(()=>pourOrder(doc.items).slice(0,1));
  const [query,setQuery]=useState(''),[error,setError]=useState(''),[busy,setBusy]=useState(false);
  const order=pourOrder(draft), selectedId=selected[0], zone=draft.get(selectedId);
  const current=zone?.kind==='zone'?zone:null;
  const close=()=>useModal.getState().close();
  const set=(patch:Partial<ZoneItem>)=>{
    if(!current)return;
    if(current.locked && Object.keys(patch).some(k=>k!=='locked')){setError('Unlock the pour before editing.');return;}
    setError('');setDraft(new Map(draft).set(selectedId,{...current,...patch}));
  };
  const choose=(id:ItemId)=>{
    setSelected([id]);useEditor.getState().patchDoc(docId,{marked:new Set([id])});
  };
  const zoom=()=>{
    if(!current?.outline.length)return;
    const xs=current.outline.map(p=>p.x),ys=current.outline.map(p=>p.y);
    useEditor.getState().patchDoc(docId,{selection:new Set([selectedId]),focus:{pos:{x:Math.round((Math.min(...xs)+Math.max(...xs))/2),y:Math.round((Math.min(...ys)+Math.max(...ys))/2)},seq:Date.now()}});
  };
  const move=(direction:-1|1)=>{try{const changes=reorderPours(draft,selectedId,direction);setDraft(new Map([...draft,...changes.map(r=>[r.id,r.item] as const)]));}catch(e){setError(String(e));}};
  const apply=async(refill:'selected'|'all'|null)=>{
    if(doc.items!==baseline){setError('The board changed while this dialog was open. Reopen the manager before applying.');return;}
    const replace=order.flatMap(id=>{const item=draft.get(id)!;return JSON.stringify(item)!==JSON.stringify(baseline.get(id))?[{id,item}]:[];});
    if(!replace.length && !refill)return close();
    if(refill==='selected'&&!selected.length)return;
    setBusy(true);
    try{if(await execute(docId,{op:'editPours',items:replace,refill:refill==='all'?null:selected}))close();else setError('Changes were not applied. See Messages.');}finally{setBusy(false);}
  };
  const length=(key:'clearance'|'minWidth'|'thermalGap'|'spokeWidth',label:string)=><label key={key} className="grid gap-1 text-xs">{label} (mm)<Input type="number" min="0" step="0.05" value={current?current[key]/1e6:0} disabled={current?.locked} onChange={e=>{if(e.target.value!==''&&Number.isFinite(e.target.valueAsNumber)&&e.target.valueAsNumber>=0)set({[key]:Math.round(e.target.valueAsNumber*1e6)});}}/></label>;
  return <Dialog wide title="Pour manager" description="Edit ordinary pours and refill overlapping copper in one undo step. Keep-outs and generated teardrops are managed separately."
    footer={<><Button variant="outline" disabled={busy} onClick={close}>Cancel</Button><Button variant="outline" disabled={busy||!selected.length} onClick={()=>void apply('selected')}>Refill selected + overlaps</Button><Button variant="outline" disabled={busy||!order.length} onClick={()=>void apply('all')}>Refill all</Button><Button disabled={busy} onClick={()=>void apply(null)}>{busy?'Applying…':'Apply changes'}</Button></>}>
    <Input placeholder="Search name, net or layer" value={query} onChange={e=>setQuery(e.target.value)}/>
    <div className="mt-3 grid gap-4 sm:grid-cols-[1fr_260px]">
      <div className="max-h-96 overflow-auto"><table className="w-full text-left text-xs"><thead><tr>{['Pour','Net','Layer','Priority','Fill','Lock'].map(x=><th key={x} className="p-1">{x}</th>)}</tr></thead><tbody>
        {order.filter(id=>{const z=draft.get(id) as ZoneItem;return [z.name,z.net,layer(z.layer).name].join(' ').toLowerCase().includes(query.toLowerCase());}).map(id=>{const z=draft.get(id) as ZoneItem;return <tr key={id} className={selected.includes(id)?'bg-sel':'hover:bg-hover'} onClick={()=>choose(id)}><td className="p-1"><button className="text-left">{z.name||'Pour '+id}</button></td><td>{z.net||'No net'}</td><td>{layer(z.layer).name}</td><td>{z.priority}</td><td>{JSON.stringify(z)!==JSON.stringify(baseline.get(id))?'Pending':z.fill.length?'Filled':'Empty'}</td><td>{z.locked?'Locked':''}</td></tr>;})}
      </tbody></table>{!order.length&&<p className="p-2 text-xs">No ordinary pours on this board.</p>}</div>
      {current&&<div className="grid content-start gap-2">
        <label className="grid gap-1 text-xs">Name<Input disabled={current.locked} value={current.name} onChange={e=>set({name:e.target.value})}/></label>
        <label className="grid gap-1 text-xs">Net<Input disabled={current.locked} value={current.net} onChange={e=>set({net:e.target.value})}/></label>
        <label className="grid gap-1 text-xs">Layer<NativeSelect disabled={current.locked} value={current.layer} onChange={e=>set({layer:Number(e.target.value)})}>{boardCopper(doc.items.values()).map(({id,name})=><NativeSelectOption key={id} value={id}>{name}</NativeSelectOption>)}</NativeSelect></label>
        <label className="grid gap-1 text-xs">Priority<Input type="number" disabled={current.locked} value={current.priority} onChange={e=>{if(Number.isInteger(e.target.valueAsNumber)&&Math.abs(e.target.valueAsNumber)<1e6)set({priority:e.target.valueAsNumber});}}/></label>
        <div className="flex gap-1"><Button size="sm" variant="outline" onClick={()=>move(-1)}>Higher</Button><Button size="sm" variant="outline" onClick={()=>move(1)}>Lower</Button><Button size="sm" variant="outline" onClick={zoom}>Zoom</Button></div>
        {length('clearance','Clearance')}{length('minWidth','Minimum copper width')}
        <label className="grid gap-1 text-xs">Pad connection<NativeSelect disabled={current.locked} value={current.connect} onChange={e=>set({connect:e.target.value as ZoneItem['connect']})}>{['thermal','solid','none'].map(x=><NativeSelectOption key={x} value={x}>{x}</NativeSelectOption>)}</NativeSelect></label>
        {length('thermalGap','Thermal gap')}{length('spokeWidth','Spoke width')}
        <label className="flex gap-2 text-xs"><Checkbox checked={current.removeIslands} disabled={current.locked} onCheckedChange={v=>set({removeIslands:v===true})}/>Remove islands</label>
        <label className="flex gap-2 text-xs"><Checkbox checked={current.locked??false} onCheckedChange={v=>set({locked:v===true})}/>Locked</label>
      </div>}
    </div><p className="mt-2 text-xs text-muted-foreground">Refill includes overlapping neighbours to keep priorities consistent. Equal priorities use item ID order. Cancel discards property changes.</p>{error&&<p role="alert" className="text-xs text-err-line">{error}</p>}
  </Dialog>;
}
