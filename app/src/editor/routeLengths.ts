import type { Item, ItemId } from '../core/types';
export interface LengthConstraint {min:number;max:number;rule:string}
export interface RoutingMetrics {nets:{net:string;length:number}[];pending:boolean;blocked:boolean}
export function copperLength(item:Item):number {
  if(item.kind==='track')return Math.hypot(item.b.x-item.a.x,item.b.y-item.a.y);
  if(item.kind==='arcTrack')return item.radius*Math.abs(item.sweep)*Math.PI/180;
  return 0;
}
const totals=new WeakMap<ReadonlyMap<ItemId,Item>,Map<string,number>>();
/** Cached per immutable board revision, not a full-board scan on each pointer move. */
export function routedLengths(items:ReadonlyMap<ItemId,Item>):ReadonlyMap<string,number> {
  let out=totals.get(items);
  if(!out){out=new Map();for(const item of items.values())if(item.kind==='track'||item.kind==='arcTrack')out.set(item.net,(out.get(item.net)??0)+copperLength(item));totals.set(items,out);}
  return out;
}
/** Replacements are subtracted before adding the accepted preview, including shoved copper. */
export function prospectiveLength(items:ReadonlyMap<ItemId,Item>,net:string,preview:readonly Item[],moved:readonly {id:ItemId;item:Item}[]):number {
  let total=routedLengths(items).get(net)??0;
  for(const {id,item} of moved){const old=items.get(id);if(old&&(old.kind==='track'||old.kind==='arcTrack')&&old.net===net)total-=copperLength(old);if((item.kind==='track'||item.kind==='arcTrack')&&item.net===net)total+=copperLength(item);}
  for(const item of preview)if((item.kind==='track'||item.kind==='arcTrack')&&item.net===net)total+=copperLength(item);
  return Math.round(Math.max(0,total));
}
export function lengthBand(length:number,bounds:{min:number;max:number}|null):'unset'|'invalid'|'below'|'within'|'above' {
  if(!bounds)return 'unset';
  if(!Number.isFinite(length)||!Number.isFinite(bounds.min)||!Number.isFinite(bounds.max)||bounds.min<0||bounds.max<bounds.min)return 'invalid';
  return length<bounds.min?'below':length>bounds.max?'above':'within';
}
/** A manual target cannot widen the board rule's permitted interval. */
export function targetBounds(rule:LengthConstraint|null,target:number|null,tolerance:number):{min:number;max:number}|null {
  if(target===null)return rule;
  return {min:Math.max(rule?.min??0,target-tolerance),max:Math.min(rule?.max??Infinity,target+tolerance)};
}
