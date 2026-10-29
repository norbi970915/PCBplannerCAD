import {useEffect,useRef,useState} from 'react';
import {open as pickFolder} from '@tauri-apps/plugin-dialog';
import {LifeBuoy,FolderOpen,ShieldCheck} from 'lucide-react';
import {Button} from '@/components/ui/button';
import {Input} from '@/components/ui/input';
import {api,type RecoveryListing,type RecoveryRestored} from '../core/api';
import {useRecoveryStatus} from '../core/recovery';
import {openAnyFile} from '../core/actions';
import {Dialog,useModal} from '../shell/modal';
import {folderNameError} from './paths';
let instance=0;
export function openRecovery(){useModal.getState().show(<RecoveryDialog key={++instance}/>);}
export function RecoveryDialog(){
  const status=useRecoveryStatus();
  const [listing,setListing]=useState<RecoveryListing>({location:'',entries:[],issues:[]}),[busy,setBusy]=useState(false),[error,setError]=useState(''),[message,setMessage]=useState(''),[name,setName]=useState('Recovered-design'),[restored,setRestored]=useState<RecoveryRestored|null>(null);
  const live=useRef(true),operation=useRef(false),listVersion=useRef(0);
  const [loading,setLoading]=useState(true);
  const refresh=async()=>{const version=++listVersion.current;if(live.current)setLoading(true);try{const list=await api.recoveryList();if(live.current&&version===listVersion.current){setListing(list);useRecoveryStatus.setState({hasCopies:!!list.entries.length});}}finally{if(live.current&&version===listVersion.current)setLoading(false);}};
  useEffect(()=>{live.current=true;void refresh().catch(e=>{if(live.current)setError(String(e));});return()=>{live.current=false;};},[]);
  const run=async(action:()=>Promise<void>)=>{if(operation.current)return;operation.current=true;setBusy(true);setError('');setMessage('');try{await action();}catch(e){if(live.current)setError(String(e));}finally{operation.current=false;if(live.current)setBusy(false);}};
  const capture=()=>run(async()=>{const info=await api.recoveryCapture(true);if(!live.current)return;setMessage(info?'Recovery copy created.':'No open native documents or editable libraries to copy.');if(info)useRecoveryStatus.setState({last:info,hasCopies:true,error:''});await refresh();});
  const restore=(id:string)=>run(async()=>{
    setRestored(null);
    const invalid=folderNameError(name);if(invalid)throw Error(invalid);
    const parent=await pickFolder({directory:true,multiple:false,title:'Choose a parent folder for the recovered copy'});if(!live.current||typeof parent!=='string')return;
    const result=await api.recoveryRestore(id,parent.replace(/[\\/]$/,'')+'/'+name);if(live.current)setRestored(result);
  });
  return <Dialog title="Recovery copies" description="Local copies of open native documents and editable libraries." wide busy={busy} footer={<Button variant="outline" disabled={busy} onClick={()=>useModal.getState().close()}>Close</Button>}>
    <div className="grid gap-3 text-xs">
      <p>Automatic recovery checks for unsaved work every minute and keeps the latest three copies per app session in local app data. Working files are preserved. These copies remain available after normal exits too.</p>
      <label className="flex items-center gap-2"><input type="checkbox" checked={status.enabled} disabled={busy} onChange={e=>status.setEnabled(e.target.checked)}/>Enable automatic recovery copies</label>
      {listing.location&&<p className="break-all text-muted-foreground">Recovery storage: {listing.location}</p>}
      <p className="text-muted-foreground">Restoring creates a new folder. Native schematic/board/library contents are included; undo history and external 3D model assets are excluded. Keep original asset folders and relink relative model paths after restoration.</p>
      <div className="flex gap-2"><Button disabled={busy} onClick={()=>void capture()}><ShieldCheck className="tone-success"/>Create recovery copy now</Button><Button variant="outline" disabled={busy} onClick={()=>void run(refresh)}>Refresh copies</Button></div>
      {status.last&&<p className="text-muted-foreground">Last copy: {new Date(status.last.createdMs).toLocaleString()}</p>}
      {status.error&&<p role="alert" className="text-err-line">Automatic recovery: {status.error}</p>}
      {error&&<p role="alert" className="text-err-line">{error}</p>}{message&&<p role="status">{message}</p>}
      <label className="grid gap-1">Recovered folder name<Input aria-label="Recovered folder name" disabled={busy} value={name} onChange={e=>setName(e.target.value)}/></label>
      {listing.issues.map((issue,i)=><p role="alert" key={i} className="text-err-line">{issue}</p>)}
      {loading&&<p role="status">Reading recovery copies…</p>}
      {!loading&&!listing.entries.length&&<p>No recovery copies found.</p>}
      {listing.entries.map(entry=><div key={entry.id} className="flex items-start gap-3 rounded border border-line bg-panel-head p-3">
        <LifeBuoy className="tone-recovery mt-0.5 size-5 shrink-0"/>
        <div className="min-w-0 flex-1"><p className="font-semibold">{entry.title}</p><p className="mt-1 text-muted-foreground">{new Date(entry.createdMs).toLocaleString()} · {entry.documents} documents · {entry.libraries} libraries · {(entry.bytes/1048576).toFixed(2)} MB</p>{entry.notes.map((note,i)=><p key={i} className="mt-1 text-muted-foreground">{note}</p>)}</div>
        <Button variant="outline" size="sm" disabled={busy||!!folderNameError(name)} onClick={()=>void restore(entry.id)}>Restore copy…</Button>
      </div>)}
      {restored&&<div role="region" aria-label="Recovered files" className="grid gap-2 rounded border border-line p-3"><p role="status" className="break-all">Recovered copy: {restored.folder}</p>{restored.notes.map((note,i)=><p key={i} className="text-muted-foreground">{note}</p>)}{restored.project&&<Button className="w-fit" disabled={busy} onClick={()=>{void openAnyFile(restored.project!);useModal.getState().close();}}><FolderOpen className="tone-folder"/>Open recovered project</Button>}{restored.files.filter(p=>!p.endsWith('.arproj')).map(path=><button key={path} className="break-all text-left text-accent-ink hover:underline" disabled={busy} onClick={()=>void openAnyFile(path)}>{path.split(/[\\/]/).pop()}</button>)}</div>}
    </div>
  </Dialog>;
}
