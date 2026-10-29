import { useEffect, useState } from 'react';
import { open as pickFolder } from '@tauri-apps/plugin-dialog';
import { Folder, FolderOpen, FileText, CircuitBoard, Library, Box, ChevronRight, Home, History, Plus, Upload, RefreshCw } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { api, type HubEntry } from '../core/api';
import { applyProjectState, openAnyFile, openProject } from '../core/actions';
import { bus } from '../core/bus';
import { useEditor } from '../core/store';
import { Dialog, useModal } from '../shell/modal';
import { openImportWizard } from './ImportWizard';
import { openRevisions } from './Revisions';
import { folderNameError } from './paths';
import { FileThumbnail,clearThumbnailCache } from './FileThumbnail';
export const WORK_ROOT='pcbplannercad.workingFolder';
export function workingFolder(){try{return localStorage.getItem(WORK_ROOT)??'';}catch{return '';}}
export const joinPath=(a:string,b:string)=>a.replace(/[\\/]$/,'')+'/'+b;
const toneFor=(kind:string)=>({folder:'folder',project:'project',schematic:'schematic',board:'board',library:'library',model:'model'}[kind]??'action');
const iconFor=(kind:string)=>({folder:Folder,project:FolderOpen,schematic:FileText,board:CircuitBoard,library:Library,model:Box}[kind]??FileText);
let hubInstance=0;
export function openLocalHub(create=false){useModal.getState().show(<HubDialog key={++hubInstance} initialCreate={create}/>);}
function HubDialog({initialCreate}:{initialCreate:boolean}){const [busy,setBusy]=useState(false);return <Dialog title="Local project hub" description="Your working folder, electronics projects and saved revisions on this computer." wide busy={busy}><LocalHub initialCreate={initialCreate} onBusyChange={setBusy}/></Dialog>;}
export function LocalHub({initialCreate=false,onBusyChange}:{initialCreate?:boolean;onBusyChange?:(busy:boolean)=>void}){
  const [root,setRoot]=useState(workingFolder),[relative,setRelative]=useState(''),[entries,setEntries]=useState<HubEntry[]>([]),[error,setError]=useState(''),[loading,setLoading]=useState(false),[busy,setBusy]=useState(false),[version,setVersion]=useState(0),[create,setCreate]=useState<'folder'|'project'|null>(initialCreate?'project':null),[name,setName]=useState('');
  const project=useEditor(s=>s.project);
  useEffect(()=>bus.on("filesSaved",()=>setVersion(v=>v+1)),[]);
  useEffect(()=>{onBusyChange?.(busy);return()=>onBusyChange?.(false);},[busy,onBusyChange]);
  useEffect(()=>{let live=true;setEntries([]);setError('');if(!root)return;setLoading(true);api.hubBrowse(root,relative).then(e=>{if(live)setEntries(e);}).catch(e=>{if(live)setError(String(e));}).finally(()=>{if(live)setLoading(false);});return()=>{live=false;};},[root,relative,version]);
  const choose=async()=>{try{const p=await pickFolder({directory:true,multiple:false,title:'Choose your working folder',defaultPath:root||undefined});if(typeof p==='string'){setRoot(p);setRelative('');try{localStorage.setItem(WORK_ROOT,p);}catch{/* preference is optional */}}}catch(e){setError(String(e));}};
  const make=async()=>{if(!root||!create)return;setBusy(true);setError('');try{if(create==='folder'){await api.hubCreateFolder(root,relative,name);setVersion(v=>v+1);}else{await applyProjectState(await api.hubCreateProject(root,relative,name));setRelative(relative?relative+'/'+name:name);const first=useEditor.getState().project?.sheets[0]?.docId;if(first)bus.emit('showDoc',{docId:first});}setCreate(null);setName('');}catch(e){setError(String(e));}finally{setBusy(false);}};
  const enter=(entry:HubEntry)=>{if(entry.kind==='folder'){setRelative(relative?relative+'/'+entry.name:entry.name);setCreate(null);}else if(entry.kind==='project'){void openProject(entry.path);}else if(['schematic','board','library'].includes(entry.kind)){void openAnyFile(entry.path);}};
  const parts=relative.split('/').filter(Boolean);
  return <div className="flex min-h-0 flex-col gap-3 p-2 text-xs">
    <div className="flex flex-wrap items-center gap-1.5"><Button size="sm" variant="outline" disabled={busy} onClick={()=>void choose()}><FolderOpen/>{root?'Working folder…':'Choose working folder…'}</Button><Button size="icon-sm" variant="ghost" disabled={!root||busy} aria-label="Refresh folders" onClick={()=>{clearThumbnailCache();setVersion(v=>v+1);}}><RefreshCw/></Button></div>
    {root&&<><p className="break-all text-[10px] text-muted-foreground" title={root}>{root}</p><nav aria-label="Folder breadcrumbs" className="flex flex-wrap items-center gap-1"><button disabled={busy} aria-label="Working folder root" className="rounded p-1 hover:bg-hover" onClick={()=>setRelative('')}><Home className="size-4"/></button>{parts.map((p,i)=><span key={i} className="flex items-center gap-1"><ChevronRight className="size-3 text-muted-foreground"/><button disabled={busy} className="rounded p-1 hover:bg-hover" onClick={()=>setRelative(parts.slice(0,i+1).join('/'))}>{p}</button></span>)}</nav>
    <div className="flex flex-wrap gap-1.5"><Button size="sm" variant="outline" disabled={busy} onClick={()=>{setCreate('folder');setName('');}}><Plus/>Folder</Button><Button size="sm" variant="outline" disabled={busy} onClick={()=>{setCreate('project');setName('');}}><CircuitBoard/>Electronics design</Button><Button size="sm" variant="outline" disabled={busy} onClick={()=>openImportWizard(joinPath(root,relative))}><Upload/>Import</Button>{project&&<Button size="sm" variant="outline" disabled={busy} onClick={openRevisions}><History/>Revisions</Button>}</div>
    {create&&<form onSubmit={e=>{e.preventDefault();void make();}} className="grid gap-2 rounded border border-line bg-panel-head p-3"><label htmlFor="hub-name">{create==='folder'?'New subfolder':'New electronics project'}</label><Input id="hub-name" placeholder={create==='folder'?'Folder name':'Project name'} autoFocus value={name} disabled={busy} onChange={e=>setName(e.target.value)}/>{create==='project'&&<p className="text-muted-foreground">Creates a project folder with schematic, board, project library and model assets.</p>}<div className="flex gap-2"><Button size="sm" disabled={busy||!!folderNameError(name)}>{busy?'Creating…':'Create'}</Button><Button type="button" size="sm" variant="ghost" disabled={busy} onClick={()=>setCreate(null)}>Cancel</Button></div></form>}
    </>}
    {!root&&<p className="py-5 text-muted-foreground">Choose a folder to organize designs into customer, product or revision subfolders. Nothing is uploaded.</p>}
    {error&&<p role="alert" className="break-words text-err-line">{error}</p>}
    {loading&&<p role="status" className="text-muted-foreground">Reading folder…</p>}
    <div className="grid gap-2" aria-label="Local files">{entries.map(e=>{const Icon=iconFor(e.kind);const opens=['folder','project','schematic','board','library'].includes(e.kind);return <button key={e.path} disabled={busy||!opens} onClick={()=>enter(e)} style={{"--file-accent":"var(--icon-"+toneFor(e.kind)+")"} as React.CSSProperties} className="pcad-file-card flex min-h-16 items-center gap-3 rounded border border-line bg-panel-head p-3 text-left hover:border-accent-ink disabled:opacity-70" title={e.path}><FileThumbnail root={root} entry={e} version={version} fallback={<Icon className={'size-8 shrink-0 tone-'+toneFor(e.kind)}/>}/><span className="min-w-0"><span className="block break-words font-medium text-ink">{e.name}</span><span className="pcad-file-kind text-[10px]">{e.kind==='project'?'Electronics design':e.kind}</span><span className="mt-1 block text-[10px] text-muted-foreground">{new Date(e.modifiedMs).toLocaleString()}</span></span></button>;})}</div>
    {root&&!loading&&!entries.length&&!error&&<p className="py-4 text-muted-foreground">This folder is empty. Create a subfolder, electronics design or import existing files.</p>}
  </div>;
}
