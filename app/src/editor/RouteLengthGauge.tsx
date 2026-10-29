import { useEffect, useMemo, useState } from 'react';
import { LengthInput } from '../components/app/fields';
import { api } from '../core/api';
import { useEditor } from '../core/store';
import { formatLength } from '../core/units';
import { lengthBand, targetBounds, type LengthConstraint, type RoutingMetrics } from './routeLengths';

export function LengthMeter({length,bounds,label,pending=false}:{length:number;bounds:{min:number;max:number}|null;label:string;pending?:boolean}) {
  const units=useEditor(s=>s.units);
  const band=lengthBand(length,bounds);
  const status=pending?'Updating':({unset:'No target',invalid:'Conflicting limits',below:'Below target',within:'In range',above:'Over target'} as const)[band];
  const color=pending||band==='unset'?'#87909f':band==='within'?'#38b66b':'#ef6262';
  const ceiling=bounds?Math.max(bounds.max*1.1,length,1):Math.max(length,1);
  return <div className="mt-1 min-w-72" title="Total routed copper per net. Tracks and arcs only; vias, pads and package transit excluded. Branched nets include all branches.">
    <div className="flex justify-between gap-4 font-mono text-[11px]"><span className="max-w-64 truncate">{label}: {formatLength(length,units)}</span><span style={{color}}>{status}</span></div>
    <div className="relative mt-1 h-2 overflow-hidden rounded bg-hover" role="meter" aria-label={label+' routed copper length'} aria-valuenow={length} aria-valuemin={0} aria-valuemax={ceiling} aria-valuetext={formatLength(length,units)+'; '+status}>
      {bounds&&band!=='invalid'&&<div className="absolute inset-y-0 border-x border-white/80 bg-white/15" style={{left:100*bounds.min/ceiling+'%',width:100*(bounds.max-bounds.min)/ceiling+'%'}}/>}
      <div className="h-full opacity-80" style={{width:Math.min(100,100*length/ceiling)+'%',backgroundColor:color}}/>
    </div>
    {bounds&&<div className="mt-0.5 text-[10px] text-muted-foreground">Allowed: {formatLength(bounds.min,units)} – {formatLength(bounds.max,units)}</div>}
  </div>;
}

export function RouteLengthGauge({docId,metrics}:{docId:string;metrics:RoutingMetrics|null}) {
  const units=useEditor(s=>s.units);
  const items=useEditor(s=>s.docs[docId]?.items);
  const board=useMemo(()=>items&&[...items.values()].find(i=>i.kind==='board'),[items]);
  const [manual,setManual]=useState(false),[target,setTarget]=useState(25e6),[tolerance,setTolerance]=useState(250000),[skewLimit,setSkewLimit]=useState(100000);
  const [resolved,setResolved]=useState<{key:string;rules:(LengthConstraint|null)[];error:string}|null>(null);
  const netsKey=JSON.stringify(metrics?.nets.map(n=>n.net)??[]);
  useEffect(()=>{let active=true;setResolved(null);const nets=JSON.parse(netsKey) as string[];
    if(nets.length)void Promise.all(nets.map(net=>api.pcbLengthConstraint(docId,net))).then(rules=>{if(active)setResolved({key:netsKey,rules,error:''});}).catch(error=>{if(active)setResolved({key:netsKey,rules:[],error:String(error)});});
    return ()=>{active=false;};
  },[docId,netsKey,board]);
  if(!metrics)return <div className="pt-1 text-[11px] text-muted-foreground">Start a route to see copper length and target feedback.</div>;
  const ready=resolved?.key===netsKey&&!resolved.error;
  const skew=metrics.nets.length===2?Math.abs(metrics.nets[0].length-metrics.nets[1].length):null;
  return <div className="mt-2 border-t border-line pt-1" onKeyDown={e=>e.stopPropagation()}>
    <div className="flex flex-wrap items-center gap-2">
      <label className="flex items-center gap-1"><input type="checkbox" checked={manual} onChange={e=>setManual(e.target.checked)}/>Manual length target</label>
      {manual&&<><label className="flex w-40 items-center gap-1">Target<LengthInput value={target} units={units} min={1} onCommit={setTarget}/></label><label className="flex w-36 items-center gap-1">±<LengthInput value={tolerance} units={units} min={0} onCommit={setTolerance}/></label></>}
    </div>
    {metrics.nets.map((n,i)=><div key={n.net}><LengthMeter label={n.net||'No net'} length={n.length} bounds={targetBounds(resolved?.rules[i]??null,manual?target:null,tolerance)} pending={metrics.pending||!ready} /><div className="text-[10px] text-muted-foreground">{resolved?.rules[i]?.rule??(ready?'No length rule':'Loading length rules…')}</div></div>)}
    {skew!==null&&<div className="mt-1 flex items-center gap-2"><span style={{color:metrics.pending?'#87909f':skew<=skewLimit?'#38b66b':'#ef6262'}}>Pair mismatch: {formatLength(skew,units)}</span><label className="flex w-40 items-center gap-1">Limit<LengthInput value={skewLimit} units={units} min={0} onCommit={setSkewLimit}/></label></div>}
    {metrics.blocked&&<div className="text-err-line">Route blocked; length shows accepted copper preview.</div>}
    {resolved?.error&&<div className="text-err-line">Length rules unavailable: {resolved.error}</div>}
    <div className="pt-1 text-[10px] text-muted-foreground">Routed copper total, including branches. Via depth, pad transit and delay excluded. Manual targets and mismatch limits are session feedback.</div>
  </div>;
}
