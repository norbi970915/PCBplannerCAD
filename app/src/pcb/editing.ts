import type { IdItem, Item, ItemId } from '../core/types';

export const isLocked = (item: Item) => 'locked' in item && item.locked === true;
export const canLock = (item: Item) => ['footprint', 'track', 'arcTrack', 'via', 'zone'].includes(item.kind);
export interface CopperPatch { width?: number; diameter?: number; drill?: number; fromLayer?: number; toLayer?: number; locked?: boolean }
/** Pure, validated replacements. Omitted values stay unchanged, including mixed values. */
export function bulkCopper(items: ReadonlyMap<ItemId, Item>, ids: Iterable<ItemId>, patch: CopperPatch): IdItem[] {
  const out: IdItem[] = [];
  const onlyLock = Object.keys(patch).every(k => k === 'locked');
  for (const id of ids) {
    const item = items.get(id); if (!item) throw new Error('Selection changed; select the objects again.');
    if (isLocked(item) && !onlyLock) throw new Error('Unlock selected objects before editing their properties.');
    let next: Item = item;
    if (item.kind === 'track' || item.kind === 'arcTrack') {
      if (patch.width !== undefined) {
        if (!Number.isSafeInteger(patch.width) || patch.width <= 0) throw new Error('Track width must be positive.');
        next = {...item, width: patch.width};
      }
    } else if (item.kind === 'via') {
      const via = {...item};
      for (const key of ['diameter','drill','fromLayer','toLayer'] as const) if (patch[key] !== undefined) via[key] = patch[key];
      if (![via.diameter, via.drill, via.fromLayer, via.toLayer].every(Number.isSafeInteger) || via.drill <= 0 || via.diameter <= via.drill || via.fromLayer === via.toLayer) throw new Error('Via diameter must exceed its positive drill; choose two different copper layers.');
      next = via;
    }
    if (patch.locked !== undefined && canLock(next)) next = {...next, locked: patch.locked} as Item;
    if (JSON.stringify(item) !== JSON.stringify(next)) out.push({id,item:next});
  }
  return out;
}

/** Stable priority order: equal priorities retain ascending item IDs. */
export function pourOrder(items: ReadonlyMap<ItemId, Item>): ItemId[] {
  return [...items].filter(([,i]) => i.kind === 'zone' && !i.keepout && !i.teardrop)
    .sort(([a,x],[b,y]) => (x.kind === 'zone' && y.kind === 'zone' ? y.priority-x.priority : 0) || a-b).map(([id])=>id);
}
export function reorderPours(items: ReadonlyMap<ItemId, Item>, id: ItemId, direction: -1 | 1): IdItem[] {
  const order=pourOrder(items), at=order.indexOf(id), to=at+direction;
  if(at<0 || to<0 || to>=order.length) return [];
  if(order.some(key=>isLocked(items.get(key)!))) throw new Error('Unlock pours before reordering priorities.');
  [order[at],order[to]]=[order[to],order[at]];
  return order.map((key,index)=>({id:key,item:{...items.get(key)!,priority:order.length-index} as Item}));
}
