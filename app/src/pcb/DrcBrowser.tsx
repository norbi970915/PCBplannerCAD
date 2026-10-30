import { useEffect, useMemo, useRef, useState } from 'react';
import { save } from '@tauri-apps/plugin-dialog';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { NativeSelect, NativeSelectOption } from '@/components/ui/native-select';
import { api } from '../core/api';
import { navigateTo, report } from '../core/actions';
import {reviewDrc} from './drcApprovals';
import {setDrcApproval} from './drcApprovalActions';
import { runDrc } from '../core/pcbActions';
import { useEditor } from '../core/store';
import { layer } from '../core/layers';
import { Dialog, useModal } from '../shell/modal';
import { filterDrc, drcCsv, type DrcFilter } from './drcReport';

export function DrcBrowser({docId}:{docId:string}){
  const doc=useEditor(s=>s.docs[docId]);
  const [filter,setFilter]=useState<DrcFilter>({query:'',severity:'all',code:'',layer:null});
  const [selected,setSelected]=useState(0),[saving,setSaving]=useState(false),[reviewing,setReviewing]=useState(false),[reviewError,setReviewError]=useState('');
  const [reviewFilter,setReviewFilter]=useState<'open'|'approved'|'all'>('open');
  const review=useMemo(()=>doc?reviewDrc(doc):null,[doc?.drc,doc?.items,doc?.footprints]);
  const rows=useMemo(()=>doc&&review?filterDrc(reviewFilter==='open'?review.open:reviewFilter==='approved'?[...review.current.keys()]:doc.drc,doc.items,filter):[],[doc?.drc,doc?.items,review,filter,reviewFilter]);
  const codes=useMemo(()=>[...new Set(doc?.drc.map(v=>v.code)??[])].sort(),[doc?.drc]);
  const layers=useMemo(()=>[...new Set(doc?.drc.flatMap(v=>v.layer===null?[]:[v.layer])??[])].sort((a,b)=>a-b),[doc?.drc]);
  const index=Math.max(0,Math.min(selected,rows.length-1)),current=rows[index];
  const listRef=useRef<HTMLDivElement>(null);
  useEffect(()=>{listRef.current?.querySelector('[aria-selected="true"]')?.scrollIntoView({block:"nearest"});},[index,filter]);
  const close=()=>useModal.getState().close();
  const show=()=>{if(!current)return;close();void navigateTo({docId,items:current.items,pos:current.pos});};
  const change=(patch:Partial<DrcFilter>)=>{setFilter({...filter,...patch});setSelected(0);};
  const exportCsv=async()=>{
    setSaving(true);
    try{const path=await save({defaultPath:'drc-report.csv',filters:[{name:'CSV report',extensions:['csv']}]});if(path)await api.writeFileBytes(path,new TextEncoder().encode(drcCsv(rows,v=>review?.current.has(v)?'Approved':'Unapproved')));}catch(e){report(e);}finally{setSaving(false);}
  };
  const approveSelected=async()=>{
    if(!current||!doc||!review)return;setReviewing(true);setReviewError('');
    const stored=review.current.get(current);
    try{if(!await setDrcApproval(docId,current,!stored,stored))setReviewError('The approval change was not saved. Check Messages and try again.');}finally{setReviewing(false);}
  };
  if(!doc)return <Dialog title="DRC results"><p>The board is no longer open.</p></Dialog>;
  return <Dialog wide title="DRC results" description="Browse the latest rule violations, filter them and jump to the affected copper. Online checks omit unrouted connections; run a full check before final review." footer={<><Button variant="outline" onClick={close}>Close</Button><Button variant="outline" disabled={!rows.length||saving} onClick={()=>void exportCsv()}>Export filtered CSV</Button><Button variant="outline" disabled={!current||reviewing||doc.drcStatus!=='current'} onClick={()=>void approveSelected()}>{current&&review?.current.has(current)?'Restore selected':'Approve selected'}</Button><Button disabled={!current} onClick={show}>Show selected</Button></>}>
    <div className="flex flex-wrap items-center gap-2 text-xs"><span className={doc.drcStatus==='current'?'text-ok':'text-note-line'}>{doc.drcStatus==='current'?'Results current':doc.drcStatus==='checking'?'Checking…':doc.drcStatus==='error'?'Check failed':doc.drcStatus==='stale'?'Results need a new check':'Not checked yet'}</span>{doc.drcCheckedAt&&<span className="text-faint">Last checked {new Date(doc.drcCheckedAt).toLocaleTimeString()}</span>}<Button size="sm" variant="outline" disabled={doc.drcStatus==='checking'} onClick={()=>void runDrc(docId)}>Run full DRC</Button></div>
    {reviewError&&<p role="alert" className="mt-2 text-xs text-err-line">{reviewError}</p>}
    {doc.drcError&&<p role="alert" className="mt-2 text-xs text-err-line">{doc.drcError}</p>}
    {doc.drcRuleErrors?.map((error,i)=><p key={i} className="mt-1 text-xs text-note-line">Rule: {error}</p>)}
    <div className="my-3 flex flex-wrap gap-2"><NativeSelect aria-label="Review status" value={reviewFilter} onChange={e=>{setReviewFilter(e.target.value as typeof reviewFilter);setSelected(0);}}><NativeSelectOption value="open">Unapproved</NativeSelectOption><NativeSelectOption value="approved">Approved</NativeSelectOption><NativeSelectOption value="all">All recorded findings</NativeSelectOption></NativeSelect><Input className="min-w-40 flex-1" placeholder="Search message, rule, net, designator or ID" value={filter.query} onChange={e=>change({query:e.target.value})}/><NativeSelect value={filter.severity} onChange={e=>change({severity:e.target.value as DrcFilter['severity']})}><NativeSelectOption value="all">All severities</NativeSelectOption><NativeSelectOption value="error">Errors</NativeSelectOption><NativeSelectOption value="warning">Warnings</NativeSelectOption></NativeSelect><NativeSelect value={filter.code} onChange={e=>change({code:e.target.value})}><NativeSelectOption value="">All issue types</NativeSelectOption>{codes.map(code=><NativeSelectOption key={code} value={code}>{code}</NativeSelectOption>)}</NativeSelect><NativeSelect value={filter.layer??'all'} onChange={e=>change({layer:e.target.value==='all'?null:Number(e.target.value)})}><NativeSelectOption value="all">All layers</NativeSelectOption>{layers.map(id=><NativeSelectOption key={id} value={id}>{layer(id).name}</NativeSelectOption>)}</NativeSelect></div>
    <div ref={listRef} tabIndex={0} onKeyDown={e=>{if(e.key==="ArrowDown"||e.key==="ArrowUp"){e.preventDefault();setSelected(Math.max(0,Math.min(Math.min(rows.length,1000)-1,index+(e.key==="ArrowDown"?1:-1))));}else if(e.key==="Enter"){e.preventDefault();show();}}} className="max-h-80 overflow-auto rounded border border-line" role="listbox" aria-label="DRC violations">{rows.slice(0,1000).map((v,i)=><button key={i} role="option" aria-selected={i===index} onClick={()=>setSelected(i)} onDoubleClick={()=>{close();void navigateTo({docId,items:v.items,pos:v.pos});}} className={'grid w-full grid-cols-[62px_110px_1fr] gap-2 border-b border-line px-2 py-1.5 text-left text-xs '+(i===index?'bg-sel':'hover:bg-hover')}><span className={v.severity==='error'?'text-err-line':'text-note-line'}>{review?.current.has(v)?'approved':v.severity}</span><span className="truncate" title={v.code}>{v.code}</span><span>{v.message}<span className="ml-2 text-faint">{v.layer===null?'':layer(v.layer).name} · #{v.items.join(', #')}</span></span></button>)}{!rows.length&&<p className="p-3 text-xs text-muted-foreground">{doc.drc.length?'No issues match the filters.':'No recorded violations. Check the result status above.'}</p>}</div>
    <div className="mt-2 flex items-center gap-2 text-xs"><span className="flex-1">{rows.length} of {doc.drc.length} issues · {review?.current.size??0} approved{rows.length>1000?' · showing the first 1,000; narrow the filters':''}</span><Button size="sm" variant="outline" disabled={!rows.length||index===0} onClick={()=>setSelected(Math.max(0,index-1))}>Previous</Button><Button size="sm" variant="outline" disabled={!rows.length||index>=Math.min(rows.length,1000)-1} onClick={()=>setSelected(index+1)}>Next</Button></div>
  </Dialog>;
}
