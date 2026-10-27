import { useMemo,useRef,useState } from 'react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { NativeSelect,NativeSelectOption } from '@/components/ui/native-select';
import { Panel,PanelHeading } from '@/components/app/panel';
import { navigateTo } from '../core/actions';
import { bus } from '../core/bus';
import { selectable } from '../core/selectFilter';
import { useEditor } from '../core/store';
import { componentRows,type ComponentFilter } from '../pcb/componentBrowser';

export function BoardComponents(){
  const doc=useEditor(s=>s.activeDocId?s.docs[s.activeDocId]:undefined);
  const [filter,setFilter]=useState<ComponentFilter>({query:'',side:'all',lock:'all'}),[page,setPage]=useState(0);
  const anchor=useRef<number|null>(null);
  const rows=useMemo(()=>doc?componentRows(doc.items,filter):[],[doc?.items,filter]);
  const pages=Math.max(1,Math.ceil(rows.length/100)),currentPage=Math.min(page,pages-1),shown=rows.slice(currentPage*100,(currentPage+1)*100);
  if(!doc||doc.info.kind!=='pcb')return null;
  const selected=[...doc.selection].filter(id=>doc.items.get(id)?.kind==='footprint');
  const movable=selected.filter(id=>{const item=doc.items.get(id);return item?.kind==='footprint'&&!item.locked;});
  const change=(patch:Partial<ComponentFilter>)=>{setFilter({...filter,...patch});setPage(0);anchor.current=null;};
  const choose=(id:number,ctrl:boolean,shift:boolean)=>{
    const current=useEditor.getState().docs[doc.info.id];if(!current)return;
    const item=current.items.get(id);if(!item||!selectable('pcb',item,current)){useEditor.getState().setHint('This component is excluded by the current selection or visibility filter.');return;}
    const selection=new Set(ctrl?current.selection:[]);
    const from=anchor.current===null?-1:rows.findIndex(row=>row.id===anchor.current),to=rows.findIndex(row=>row.id===id);
    if(shift&&from>=0&&to>=0){for(const row of rows.slice(Math.min(from,to),Math.max(from,to)+1))if(selectable('pcb',row.item,current))selection.add(row.id);}
    else if(ctrl&&selection.has(id))selection.delete(id);else selection.add(id);
    if(!shift)anchor.current=id;
    useEditor.getState().patchDoc(doc.info.id,{selection});
  };
  const selectFiltered=()=>{useEditor.getState().patchDoc(doc.info.id,{selection:new Set(rows.filter(row=>selectable('pcb',row.item,doc)).map(row=>row.id))});};
  return <Panel><PanelHeading title="PCB · Components"/><div className="grid shrink-0 gap-1.5 border-b border-line p-2"><Input placeholder="Designator, value or footprint…" value={filter.query} onChange={e=>change({query:e.target.value})}/><div className="flex gap-1"><NativeSelect wrapperClassName="flex-1" value={filter.side} onChange={e=>change({side:e.target.value as ComponentFilter['side']})}><NativeSelectOption value="all">Both sides</NativeSelectOption><NativeSelectOption value="top">Top</NativeSelectOption><NativeSelectOption value="bottom">Bottom</NativeSelectOption></NativeSelect><NativeSelect wrapperClassName="flex-1" value={filter.lock} onChange={e=>change({lock:e.target.value as ComponentFilter['lock']})}><NativeSelectOption value="all">Any lock state</NativeSelectOption><NativeSelectOption value="locked">Locked</NativeSelectOption><NativeSelectOption value="unlocked">Unlocked</NativeSelectOption></NativeSelect></div><div className="flex flex-wrap gap-1"><Button size="sm" variant="outline" disabled={!rows.length} onClick={selectFiltered}>Select filtered</Button><Button size="sm" disabled={!movable.length} onClick={()=>bus.emit('pickUp',{docId:doc.info.id,ids:movable})}>Move {movable.length||''} parts</Button></div></div>
    <div className="grid grid-cols-[50px_1fr_46px] gap-2 border-b border-line bg-panel-head px-2 py-1 text-[10px] text-faint"><span>Ref</span><span>Value / footprint</span><span>Side</span></div>
    <div className="min-h-0 flex-1 overflow-auto" role="listbox" aria-label="Board components" aria-multiselectable="true">{shown.map(({id,item})=><button key={id} role="option" aria-selected={doc.selection.has(id)} onClick={e=>choose(id,e.ctrlKey||e.metaKey,e.shiftKey)} onDoubleClick={()=>void navigateTo({docId:doc.info.id,items:[id],pos:item.pos})} title={item.designator+' · '+item.value+' · '+item.footprint+(item.locked?' · Locked':'')+' · Double-click to zoom'} className={'grid w-full grid-cols-[50px_1fr_46px] items-start gap-2 border-b border-line/40 px-2 py-1 text-left text-xs '+(doc.selection.has(id)?'bg-sel':'hover:bg-hover')}><span className="truncate font-mono">{item.designator||'—'}</span><span className="min-w-0"><span className="block truncate">{item.value||'—'}</span><span className="block truncate text-[10px] text-faint">{item.footprint}</span></span><span className="text-[10px] text-faint">{item.side}{item.locked&&<span className="block">Locked</span>}</span></button>)}{!rows.length&&<p className="p-3 text-xs text-faint">No components match.</p>}</div>
    <div className="flex shrink-0 items-center gap-1 border-t border-line p-2 text-[10px]"><span className="flex-1">{rows.length} components · {selected.length} selected</span><Button size="sm" variant="ghost" disabled={!currentPage} onClick={()=>setPage(currentPage-1)}>‹</Button><span>{currentPage+1}/{pages}</span><Button size="sm" variant="ghost" disabled={currentPage>=pages-1} onClick={()=>setPage(currentPage+1)}>›</Button></div><p className="shrink-0 px-2 pb-2 text-[10px] text-faint">Ctrl-click toggles; Shift-click selects a range. Double-click zooms. Locked parts stay in place.</p>
  </Panel>;
}
