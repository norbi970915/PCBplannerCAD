import {useEffect,useRef,useState} from 'react';
import {open as pickFolder,save as pickExport} from '@tauri-apps/plugin-dialog';
import {Button} from '@/components/ui/button';
import {Input} from '@/components/ui/input';
import {api,type RevisionInfo,type RevisionChanges} from '../core/api';
import {openProject,saveAll} from '../core/actions';
import {useEditor} from '../core/store';
import {Dialog,useModal} from '../shell/modal';
import {folderNameError} from './paths';
import {revisionChangesText} from './revisionText';
let historyInstance=0;
export function openRevisions(){useModal.getState().show(<Revisions key={++historyInstance}/>);}
export function Revisions(){
  const project=useEditor(s=>s.project);
  const [list,setList]=useState<RevisionInfo[]>([]),[label,setLabel]=useState(''),[name,setName]=useState((project?.name??'Project')+'-restored'),[busy,setBusy]=useState(false),[error,setError]=useState(''),[restored,setRestored]=useState('');
  const [from,setFrom]=useState(''),[to,setTo]=useState(''),[comparison,setComparison]=useState<{key:string;data:RevisionChanges}|null>(null);
  const live=useRef(true),operation=useRef<symbol|null>(null);
  const [exported,setExported]=useState('');
  const current=()=>live.current&&useEditor.getState().project===project;
  useEffect(()=>{live.current=true;return()=>{live.current=false;operation.current=null;};},[]);
  useEffect(()=>{
    let active=true;operation.current=null;setBusy(false);setList([]);setComparison(null);setError('');setRestored('');setExported('');setLabel('');setName((project?.name??'Project')+'-restored');setFrom('');setTo('');
    if(project)void api.projectRevisions(project.path).then(r=>{if(active&&useEditor.getState().project===project){setList(r);setTo(r[0]?.id??'');setFrom(r[1]?.id??r[0]?.id??'');}}).catch(e=>{if(active&&useEditor.getState().project===project)setError(String(e));});
    return()=>{active=false;};
  },[project]);
  const key=JSON.stringify([project?.path,from,to]);const shown=comparison?.key===key?comparison.data:null;
  const begin=()=>{if(operation.current||!project||!current())return null;const token=Symbol();operation.current=token;setBusy(true);setError('');setExported('');return token;};
  const valid=(token:symbol)=>current()&&operation.current===token;
  const finish=(token:symbol)=>{if(valid(token)){operation.current=null;setBusy(false);}};
  const checkpoint=async()=>{
    const token=begin();if(!token||!project)return;
    try{
      const saved=await saveAll();if(!valid(token))return;
      if(!saved){setError('Save was cancelled or failed. No checkpoint was created.');return;}
      const revision=await api.projectCheckpoint(project.path,label);if(!valid(token))return;
      const revisions=await api.projectRevisions(project.path);if(!valid(token))return;
      setFrom(list[0]?.id??revision.id);setTo(revision.id);setComparison(null);setList(revisions);setLabel('');
    }catch(e){if(valid(token))setError(String(e));}finally{finish(token);}
  };
  const compare=async()=>{
    const token=begin();if(!token||!project)return;setComparison(null);
    try{const data=await api.projectCompareRevisions(project.path,from,to);if(valid(token))setComparison({key,data});}catch(e){if(valid(token))setError(String(e));}finally{finish(token);}
  };
  const restore=async(id:string)=>{
    const invalid=folderNameError(name);if(invalid){setError(invalid);return;}const token=begin();if(!token||!project)return;
    try{const parent=await pickFolder({directory:true,multiple:false,title:'Choose a parent folder for the restored copy'});if(!valid(token)||typeof parent!=='string')return;
      const restoredPath=await api.projectRestoreCopy(project.path,id,parent.replace(/[\\/]$/,'')+'/'+name);if(valid(token))setRestored(restoredPath);
    }catch(e){if(valid(token))setError(String(e));}finally{finish(token);}
  };
  const exportChanges=async()=>{
    if(!shown)return;const text=revisionChangesText(shown),token=begin();if(!token)return;
    try{const path=await pickExport({defaultPath:'pCAD-changelog.txt',filters:[{name:'Text changelog',extensions:['txt']}]});if(!valid(token)||!path)return;
      await api.writeFileBytes(path,new TextEncoder().encode(text));if(valid(token))setExported(path);
    }catch(e){if(valid(token))setError(String(e));}finally{finish(token);}
  };
  const revisionLabel=(r:RevisionInfo)=>(r.label||'Checkpoint')+' · '+new Date(r.createdMs).toLocaleString();
  return <Dialog title="Project revisions" description={project?'Saved checkpoints for '+project.name:'Open a project first.'} wide busy={busy} footer={<Button variant="outline" disabled={busy} onClick={()=>useModal.getState().close()}>Close</Button>}>
    <div className="grid gap-4 text-xs">
      <p className="text-muted-foreground">Normal Save updates the working files. Use Save &amp; checkpoint for each saved version you want in history. Compare shows changes between those snapshots; unsaved edits are excluded.</p>
      <div className="flex gap-2"><Input aria-label="Checkpoint note" placeholder="Checkpoint note, e.g. Before routing" value={label} disabled={busy||!project} onChange={e=>setLabel(e.target.value)}/><Button disabled={busy||!project} onClick={()=>void checkpoint()}>Save &amp; checkpoint</Button></div>
      {list.length>0&&<section className="grid gap-2 rounded border border-line bg-panel-head p-3">
        <h3 className="font-semibold">Changes between saved versions</h3>
        <div className="grid gap-2 sm:grid-cols-2">
          <label className="grid gap-1">From checkpoint<select aria-label="From checkpoint" className="min-w-0 rounded border border-line bg-card p-2" value={from} disabled={busy} onChange={e=>setFrom(e.target.value)}>{list.map(r=><option key={r.id} value={r.id}>{revisionLabel(r)}</option>)}</select></label>
          <label className="grid gap-1">To checkpoint<select aria-label="To checkpoint" className="min-w-0 rounded border border-line bg-card p-2" value={to} disabled={busy} onChange={e=>setTo(e.target.value)}>{list.map(r=><option key={r.id} value={r.id}>{revisionLabel(r)}</option>)}</select></label>
        </div>
        <Button className="w-fit" variant="outline" disabled={busy||!from||!to} onClick={()=>void compare()}>Compare saved versions</Button>
        {shown&&<div role="region" aria-label="Saved changes" className="grid gap-2"><Button className="w-fit" variant="outline" disabled={busy} onClick={()=>void exportChanges()}>Export changelog…</Button><p role="status">{shown.added} files added · {shown.removed} removed · {shown.modified} modified</p>{!shown.files.length&&<p>No saved file changes between these checkpoints.</p>}
          {shown.files.map(file=><details key={file.path} open className="rounded border border-line bg-card p-2"><summary className="break-all font-medium">{file.status} · {file.path}</summary><p className="mt-1 text-muted-foreground">{file.summary}</p>{file.details.length>0&&<ul className="mt-2 max-h-56 list-inside list-disc overflow-auto break-words">{file.details.map((detail,i)=><li key={i}>{detail}</li>)}</ul>}{file.omitted>0&&<p className="mt-1 text-muted-foreground">{file.omitted} additional detail lines omitted from this view.</p>}</details>)}
        </div>}
      </section>}
      <label className="grid gap-1">Restored folder name<Input value={name} disabled={busy} onChange={e=>setName(e.target.value)}/></label>
      {exported&&<p role="status" className="break-all">Exported changelog: {exported}</p>}
      {error&&<p role="alert" className="text-err-line">{error}</p>}
      {restored&&<div className="rounded border border-line p-3"><p className="break-all">Restored copy: {restored}</p><Button className="mt-2" disabled={busy} onClick={()=>{void openProject(restored);useModal.getState().close();}}>Open restored copy</Button></div>}
      <div className="grid gap-2">{list.map(r=><div key={r.id} className="flex items-center gap-3 rounded border border-line bg-panel-head p-3"><div className="min-w-0 flex-1"><p className="break-words font-medium">{r.label||'Checkpoint'}</p><p className="mt-1 text-muted-foreground">{new Date(r.createdMs).toLocaleString()} · {r.files} files · {(r.bytes/1048576).toFixed(2)} MB</p></div><Button size="sm" variant="outline" disabled={busy||!!folderNameError(name)} onClick={()=>void restore(r.id)}>Restore copy…</Button></div>)}</div>
      {!list.length&&<p className="text-muted-foreground">No saved checkpoints yet.</p>}
    </div>
  </Dialog>;
}
