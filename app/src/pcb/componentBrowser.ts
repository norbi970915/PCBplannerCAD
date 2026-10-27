import type { Item,ItemId,PlacedFootprintItem } from '../core/types';
const natural=new Intl.Collator(undefined,{numeric:true,sensitivity:'base'});
export interface ComponentFilter {query:string;side:'all'|'top'|'bottom';lock:'all'|'locked'|'unlocked'}
export function componentRows(items:ReadonlyMap<ItemId,Item>,filter:ComponentFilter):{id:ItemId;item:PlacedFootprintItem}[]{
  const query=filter.query.trim().toLowerCase(),out:{id:ItemId;item:PlacedFootprintItem}[]=[];
  for(const[id,item]of items){
    if(item.kind!=='footprint'||filter.side!=='all'&&item.side!==filter.side||filter.lock==='locked'&&!item.locked||filter.lock==='unlocked'&&item.locked)continue;
    if(query&&![item.designator,item.value,item.footprint].some(value=>value.toLowerCase().includes(query)))continue;
    out.push({id,item});
  }
  return out.sort((a,b)=>natural.compare(a.item.designator,b.item.designator)||a.id-b.id);
}
