import {useState} from 'react';
import {Button} from '@/components/ui/button';
import {Input} from '@/components/ui/input';
import {CollapsibleSection} from '@/components/app/panel';
import {useEditor,type DocState} from '../core/store';
import {displaySettings,readViewPresets,saveViewPreset,viewPresetsKey,type ViewPreset} from '../core/viewPresets';

export function ViewPresets({doc}:{doc:DocState}){
  const [presets,setPresets]=useState(()=>readViewPresets(doc.info.kind));
  const [selected,setSelected]=useState(''),[name,setName]=useState(''),[error,setError]=useState('');
  const write=(next:ViewPreset[])=>{localStorage.setItem(viewPresetsKey(doc.info.kind),JSON.stringify(next));setPresets(next);setError('');};
  const save=(replace=false)=>{try{
    const current=useEditor.getState(),live=current.docs[doc.info.id];if(!live)return;
    const preset={name:replace?selected:name,display:displaySettings(live.view),layerMode:live.layerMode,pourFills:current.showPourFills};
    write(saveViewPreset(presets,preset,replace));setSelected(preset.name.trim());setName('');
  }catch(e){setError(String(e));}};
  const apply=()=>{
    const preset=presets.find(p=>p.name===selected),state=useEditor.getState();if(!preset||!state.docs[doc.info.id])return;
    state.setView(doc.info.id,preset.display);state.patchDoc(doc.info.id,{layerMode:preset.layerMode});
    if(state.showPourFills!==preset.pourFills)state.togglePourFills();setError('');
  };
  const remove=()=>{try{write(presets.filter(p=>p.name!==selected));setSelected('');}catch(e){setError(String(e));}};
  return <CollapsibleSection title="Saved views" count={presets.length} preferenceKey="pcbplannercad.sidebar.savedViewsExpanded">
    <div className="grid gap-2 text-[11px]">
      <label className="grid gap-1">View preset<select aria-label="View preset" className="min-w-0 rounded border border-line bg-card p-1.5" value={selected} onChange={e=>{setSelected(e.target.value);setError('');}}><option value="">Choose a saved view</option>{presets.map(p=><option key={p.name} value={p.name}>{p.name}</option>)}</select></label>
      <div className="flex flex-wrap gap-1.5"><Button variant="outline" size="sm" disabled={!selected} onClick={apply}>Apply view</Button><Button variant="outline" size="sm" disabled={!selected} onClick={()=>save(true)}>Replace selected</Button><Button variant="outline" size="sm" disabled={!selected} onClick={remove}>Delete preset</Button></div>
      <Input aria-label="New view preset name" maxLength={60} placeholder="New view name" value={name} onChange={e=>setName(e.target.value)} onKeyDown={e=>{if(e.key==='Enter'&&name.trim()){e.preventDefault();save();}}}/>
      <Button className="w-fit" variant="outline" size="sm" disabled={!name.trim()} onClick={()=>save()}>Save new view</Button>
      <p className="text-muted-foreground">Remembers layers, labels, object visibility, layer mode and pour fills on this PC for this editor type. Grid, snapping and the active layer keep their current settings.</p>
      {error&&<p role="alert" className="text-err-line">{error}</p>}
    </div>
  </CollapsibleSection>;
}
