import { useState } from 'react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { NativeSelect, NativeSelectOption } from '@/components/ui/native-select';
import { Section } from '@/components/app/panel';
import { execute } from '../core/actions';
import type { DocState } from '../core/store';
import { bulkCopper, canLock, isLocked, type CopperPatch } from '../pcb/editing';
import { boardCopper } from '../core/layers';

export function BulkProperties({doc}:{doc:DocState}) {
  const [fields,setFields]=useState<Record<string,string>>({});
  const [error,setError]=useState(''),[busy,setBusy]=useState(false);
  const selected=[...doc.selection].flatMap(id=>doc.items.get(id) ? [doc.items.get(id)!] : []);
  const tracks=selected.filter(i=>i.kind==='track'||i.kind==='arcTrack'),vias=selected.filter(i=>i.kind==='via');
  if(!selected.some(canLock))return null;
  const apply=async(lock?:boolean)=>{
    setError('');
    try {
      const patch:CopperPatch={};
      if(lock!==undefined) patch.locked=lock;
      else for(const key of ['width','diameter','drill','fromLayer','toLayer'] as const) {
        if(!fields[key]?.trim())continue;
        const number=Number(fields[key]); if(!Number.isFinite(number))throw new Error('Enter a valid number.');
        patch[key]=key.endsWith('Layer')?number:Math.round(number*1e6);
      }
      const items=bulkCopper(doc.items,doc.selection,patch);
      if(!items.length)return;
      setBusy(true);
      if(await execute(doc.info.id,{op:'route',delete:[],add:[],replace:items}))setFields({});
    } catch(e) {setError(String(e));} finally {setBusy(false);}
  };
  const field=(key:string,label:string)=> <label className="grid gap-1 py-1 text-xs" key={key}>{label}<Input value={fields[key]??''} placeholder="Mixed / unchanged" onChange={e=>setFields({...fields,[key]:e.target.value})}/></label>;
  return <Section title="Copper properties & locking">
    <p className="text-xs text-muted-foreground">{selected.filter(isLocked).length} locked. Blank fields keep existing values. Lengths in mm.</p>
    {tracks.length>0&&field('width','Track width')}
    {vias.length>0&&<>{field('diameter','Via diameter')}{field('drill','Via drill')}{(['fromLayer','toLayer'] as const).map(key=><label key={key} className="grid gap-1 py-1 text-xs">{key==='fromLayer'?'Via start layer':'Via end layer'}<NativeSelect value={fields[key]??''} onChange={e=>setFields({...fields,[key]:e.target.value})}><NativeSelectOption value="">Unchanged</NativeSelectOption>{boardCopper(doc.items.values()).map(({id,name})=><NativeSelectOption key={id} value={id}>{name}</NativeSelectOption>)}</NativeSelect></label>)}</>}
    <div className="flex flex-wrap gap-1 pt-2">
      {(tracks.length>0||vias.length>0)&&<Button size="sm" disabled={busy} onClick={()=>void apply()}>Apply properties</Button>}
      <Button variant="outline" size="sm" disabled={busy} onClick={()=>void apply(true)}>Lock</Button>
      <Button variant="outline" size="sm" disabled={busy} onClick={()=>void apply(false)}>Unlock</Button>
    </div>{error&&<p role="alert" className="text-xs text-err-line">{error}</p>}
  </Section>;
}
