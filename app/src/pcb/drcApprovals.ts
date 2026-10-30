import type {BoardItem, DrcApproval, DrcViolation, Footprint, Item, ItemId} from '../core/types';

type ReviewSource={items:ReadonlyMap<ItemId,Item>;footprints:Record<string,Footprint>;drc:readonly DrcViolation[]};
/** Compare structured values independently of object-property insertion order. */
export function stableDrcValue(value:unknown):string {
  return JSON.stringify(value,(_key,v)=>v&&typeof v==='object'&&!Array.isArray(v)
    ? Object.fromEntries(Object.keys(v).sort().map(key=>[key,v[key]])) : v);
}
export function boardDrcInputs(board:BoardItem):unknown {
  const {drcApprovals:_approvals,...inputs}=board;
  return inputs;
}
/** Approval metadata does not change geometry, rules or the validity of a completed check. */
export function approvalOnlyChange(previous:ReadonlyMap<ItemId,Item>,next:ReadonlyMap<ItemId,Item>):boolean {
  if(previous.size!==next.size)return false;
  let changed=false;
  for(const[id,item]of next){
    const old=previous.get(id);if(old===item)continue;
    if(old?.kind!=='board'||item.kind!=='board'||stableDrcValue(boardDrcInputs(old))!==stableDrcValue(boardDrcInputs(item)))return false;
    changed=true;
  }
  return changed;
}
export function normalizedFinding(finding:DrcViolation):DrcViolation {
  return {...finding,items:[...new Set(finding.items)].sort((a,b)=>a-b)};
}
export const findingKey=(finding:DrcViolation)=>stableDrcValue(normalizedFinding(finding));

function contextReader(doc:ReviewSource) {
  const board=[...doc.items.values()].find((item):item is BoardItem=>item.kind==='board');
  const base=board?boardDrcInputs(board):null;
  const cache=new Map<string,string>();
  return (finding:DrcViolation)=>{
    const ids=[...new Set(finding.items)].sort((a,b)=>a-b),key=JSON.stringify(ids);
    const existing=cache.get(key);if(existing!==undefined)return existing;
    // Findings without object IDs may depend on global geometry, e.g. a missing outline.
    const related=ids.length?ids.map(id=>[id,doc.items.get(id)??null] as const)
      :[...doc.items].filter(([,item])=>item.kind!=='board').sort(([a],[b])=>a-b);
    const footprints=Object.fromEntries(related.flatMap(([,item])=>item?.kind==='footprint'?[[item.footprint,doc.footprints[item.footprint]??null]]:[]));
    const context=stableDrcValue({schema:1,board:base,items:related.map(([id,item])=>[id,item?.kind==='board'?boardDrcInputs(item):item]),footprints});
    cache.set(key,context);return context;
  };
}
export function makeDrcApproval(doc:ReviewSource,finding:DrcViolation):DrcApproval {
  return {finding:normalizedFinding(finding),context:contextReader(doc)(finding)};
}
export interface ReviewedApproval {approval:DrcApproval;state:'current'|'changed'|'notReported'}
export interface DrcReview {open:DrcViolation[];approved:ReviewedApproval[];current:ReadonlyMap<DrcViolation,DrcApproval>}
const cache=new WeakMap<ReadonlyMap<ItemId,Item>,{footprints:ReviewSource['footprints'];findings:ReviewSource['drc'];review:DrcReview}>();
export function reviewDrc(doc:ReviewSource):DrcReview {
  const previous=cache.get(doc.items);if(previous?.footprints===doc.footprints&&previous.findings===doc.drc)return previous.review;
  const approvals=[...doc.items.values()].flatMap(item=>item.kind==='board'?item.drcApprovals??[]:[]);
  const open:DrcViolation[]=[],current=new Map<DrcViolation,DrcApproval>();
  const context=contextReader(doc),byFinding=new Map(approvals.map(approval=>[findingKey(approval.finding),approval]));
  for(const finding of doc.drc){const approval=byFinding.get(findingKey(finding));if(approval&&approval.context===context(finding))current.set(finding,approval);else open.push(finding);}
  const matched=new Set(current.values());
  const approved=approvals.map(approval=>({approval,state:matched.has(approval)?'current' as const:approval.context===context(approval.finding)?'notReported' as const:'changed' as const}));
  const review={open,approved,current};cache.set(doc.items,{footprints:doc.footprints,findings:doc.drc,review});return review;
}
