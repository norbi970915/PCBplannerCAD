import {useEffect,useMemo,useState} from 'react';
import {Button} from '@/components/ui/button';
import {Input} from '@/components/ui/input';
import {NativeSelect,NativeSelectOption} from '@/components/ui/native-select';
import {navigateTo} from '../core/actions';
import {runDrc} from '../core/pcbActions';
import {useEditor,type DocState} from '../core/store';
import type {DrcApproval,DrcViolation} from '../core/types';
import {layer} from '../core/layers';
import {filterDrc} from '../pcb/drcReport';
import {reviewDrc} from '../pcb/drcApprovals';
import {setDrcApproval} from '../pcb/drcApprovalActions';
import {openDialog} from '../pcb/dialogs';

const PAGE_SIZE=100;
export function DrcPanel({doc}:{doc:DocState}) {
  const [query,setQuery]=useState(''),[severity,setSeverity]=useState<'all'|'error'|'warning'>('all');
  const [page,setPage]=useState(0),[approvedPage,setApprovedPage]=useState(0),[busy,setBusy]=useState(false),[error,setError]=useState('');
  useEffect(()=>{setQuery('');setSeverity('all');setPage(0);setApprovedPage(0);},[doc.drcRunId]);
  const review=useMemo(()=>reviewDrc(doc),[doc.items,doc.footprints,doc.drc]);
  const filter={query,severity,code:'',layer:null};
  const rows=filterDrc(review.open,doc.items,filter);
  const approved=review.approved.filter(row=>filterDrc([row.approval.finding],doc.items,filter).length);
  const pages=Math.max(1,Math.ceil(rows.length/PAGE_SIZE)),current=Math.min(page,pages-1),start=current*PAGE_SIZE;
  const approvedPages=Math.max(1,Math.ceil(approved.length/PAGE_SIZE)),approvedCurrent=Math.min(approvedPage,approvedPages-1),approvedStart=approvedCurrent*PAGE_SIZE;
  const errors=review.open.filter(v=>v.severity==='error').length,warnings=review.open.length-errors;
  const status=doc.drcStatus==='checking'?'Checking…':doc.drcStatus==='error'?'Check failed':doc.drcStatus==='stale'?'Board changed — recheck':doc.drcStatus==='unchecked'?'Not checked':doc.drcScope==='online'?'Online findings':'Full check complete';
  const open=(name:'rules'|'drcResults')=>{useEditor.getState().setActiveDoc(doc.info.id);openDialog(name);};
  const approve=async(finding:DrcViolation,value:boolean,stored?:DrcApproval)=>{
    setBusy(true);setError('');
    try{if(!await setDrcApproval(doc.info.id,finding,value,stored))setError('The approval change was not saved. Check Messages and try again.');}finally{setBusy(false);}
  };
  const finding=(v:DrcViolation)=><button type="button" className="grid w-full min-w-0 gap-1 text-left hover:bg-hover focus-visible:outline-2 focus-visible:outline-accent-ink" onClick={()=>void navigateTo({docId:doc.info.id,items:v.items.filter(id=>doc.items.has(id)),pos:v.pos})}>
    <span className="flex min-w-0 items-start gap-2"><span className={v.severity==='error'?'text-err-line':'text-note-line'}>{v.severity==='error'?'Error':'Warning'}</span><span className="min-w-0 break-all font-semibold">{v.code}</span></span>
    <span className="break-words">{v.message}</span><span className="break-words text-[11px] text-muted-foreground">{[v.rule,v.layer===null?'':layer(v.layer).name,v.items.length?'#'+v.items.join(', #'):''].filter(Boolean).join(' · ')}</span>
  </button>;
  return <section aria-label="Board DRC findings" className="grid min-w-0 content-start gap-2 p-2.5 text-xs">
    <div className="flex flex-wrap items-center gap-x-3 gap-y-1"><strong className={errors?'text-err-line':'text-ink'}>{errors} errors</strong><span className={warnings?'text-note-line':'text-muted-foreground'}>{warnings} warnings</span><span className="text-muted-foreground">{review.current.size} approved in this check</span></div>
    <p role="status" className={doc.drcStatus==='current'?'text-muted-foreground':'text-note-line'}>{status}</p>
    {doc.drcError&&<p role="alert" className="break-words text-err-line">{doc.drcError}</p>}
    {error&&<p role="alert" className="break-words text-err-line">{error}</p>}
    {!!doc.drcRuleErrors.length&&<div role="alert" className="rounded border border-err-line p-2"><strong>Rules need attention</strong><ul className="mt-1 grid gap-1">{doc.drcRuleErrors.map((error,i)=><li className="break-words" key={i}>{error}</li>)}</ul></div>}
    <div className="flex flex-wrap gap-1"><Button size="sm" disabled={busy||doc.drcStatus==='checking'} onClick={()=>void runDrc(doc.info.id)}>Run DRC</Button><Button size="sm" variant="outline" onClick={()=>open('rules')}>Design rules…</Button><Button size="sm" variant="ghost" onClick={()=>open('drcResults')}>Full report…</Button></div>
    <p className="text-[11px] leading-relaxed text-muted-foreground">Click a finding to select its objects. Approvals are saved with the board and can be undone. Changed geometry or rules need approval again. Run a full check to include unrouted connections.</p>
    <Input aria-label="Search DRC findings" placeholder="Search findings, nets, components…" value={query} onChange={e=>{setQuery(e.target.value);setPage(0);setApprovedPage(0);}}/>
    <NativeSelect aria-label="DRC severity" value={severity} onChange={e=>{setSeverity(e.target.value as typeof severity);setPage(0);setApprovedPage(0);}}><NativeSelectOption value="all">All findings</NativeSelectOption><NativeSelectOption value="error">Errors</NativeSelectOption><NativeSelectOption value="warning">Warnings</NativeSelectOption></NativeSelect>
    <details className="rounded border border-line p-2"><summary className="cursor-pointer font-semibold">Approved ({review.approved.length})</summary>
      <div role="list" aria-label="Approved DRC findings" className="mt-2 grid gap-2">{approved.slice(approvedStart,approvedStart+PAGE_SIZE).map(({approval,state},i)=><div role="listitem" key={approvedStart+i} className="grid gap-1 border-t border-line pt-2">{finding(approval.finding)}<p className="text-[11px] text-muted-foreground">{state==='current'?'Approved for this geometry and rules.':state==='changed'?'Board changed — this approval no longer applies.':'No matching finding in the current results.'}</p><Button size="sm" variant="outline" disabled={busy} onClick={()=>void approve(approval.finding,false,approval)}>Restore</Button></div>)}</div>
      {!approved.length&&<p className="mt-2 text-muted-foreground">{review.approved.length?'No approved findings match the filters.':'No approved findings.'}</p>}
      {approvedPages>1&&<div className="mt-2 flex flex-wrap items-center gap-1"><span className="flex-1">{approvedStart+1}–{Math.min(approvedStart+PAGE_SIZE,approved.length)} of {approved.length}</span><Button size="sm" variant="outline" disabled={!approvedCurrent} onClick={()=>setApprovedPage(approvedCurrent-1)}>Previous approved</Button><Button size="sm" variant="outline" disabled={approvedCurrent===approvedPages-1} onClick={()=>setApprovedPage(approvedCurrent+1)}>Next approved</Button></div>}
    </details>
    <div role="list" aria-label="DRC findings" className="grid gap-1">{rows.slice(start,start+PAGE_SIZE).map((v,i)=><div role="listitem" key={start+i} className="grid min-w-0 gap-1 rounded border border-line p-2">{finding(v)}<Button size="sm" variant="outline" disabled={busy||doc.drcStatus!=='current'} onClick={()=>void approve(v,true)}>Approve</Button></div>)}</div>
    {!rows.length&&<p className="rounded border border-line p-2 text-muted-foreground">{review.open.length?'No findings match the filters.':doc.drcStatus==='current'&&!doc.drcRuleErrors.length?(review.current.size?'No unapproved findings in this check.':'No violations reported for this check.'):'No findings to show. Check the status above.'}</p>}
    {!!rows.length&&<div className="flex flex-wrap items-center justify-between gap-1"><span>{start+1}–{Math.min(start+PAGE_SIZE,rows.length)} of {rows.length}</span>{pages>1&&<div className="flex gap-1"><Button size="sm" variant="outline" disabled={!current} onClick={()=>setPage(current-1)}>Previous</Button><Button size="sm" variant="outline" disabled={current===pages-1} onClick={()=>setPage(current+1)}>Next</Button></div>}</div>}

  </section>;
}
