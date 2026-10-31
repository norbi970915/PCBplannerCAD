import { L } from './layers';
import type { Item,LayerId } from './types';
import type { LayerMode,ViewSettings } from './view';

/** Copper order: top, inner layers by number, bottom. */
const copperRank = (id: LayerId) => (id === L.topCopper ? 0 : id === L.bottomCopper ? 1000 : id - 32);

/** True when the item touches at least one layer the filter allows. */
export function onSelectableLayer(item: Item, offLayers: readonly LayerId[]): boolean {
  if (!offLayers.length) return true;
  const off = new Set(offLayers);
  const anyCopper = (lo: number, hi: number) => {
    for (const id of [L.topCopper, L.bottomCopper, ...Array.from({ length: 30 }, (_, i) => 33 + i)]) {
      const r = copperRank(id);
      if (r >= lo && r <= hi && !off.has(id)) return true;
    }
    return false;
  };
  if (item.kind === "via") {
    const [a, b] = [copperRank(item.fromLayer), copperRank(item.toLayer)];
    return anyCopper(Math.min(a, b), Math.max(a, b));
  }
  // Components belong to the copper layer of their side.
  if (item.kind === "footprint") return !off.has(item.side === "bottom" ? L.bottomCopper : L.topCopper);
  if ("layer" in item && typeof item.layer === "number") {
    if (item.layer === L.multi) return anyCopper(0, 1000);
    return !off.has(item.layer);
  }
  return true;
}

const ALL_LAYERS: LayerId[] = Array.from({ length: 63 }, (_, i) => i);

/** Isolated views pick physical objects on exactly the active layer. */
export function onActiveLayer(item:Item,active:LayerId):boolean {
  const copper=active===L.topCopper||active===L.bottomCopper||(active>=33&&active<63);
  if(item.kind==='via')return copper&&onSelectableLayer(item,ALL_LAYERS.filter(id=>id!==active));
  // A component container includes geometry from several layers. Use all-layer
  // view for whole-component moves; isolated copper never grabs its body/text.
  if(item.kind==='footprint')return false;
  if('layer' in item){if(item.layer===active)return true;return item.kind==='pad'&&item.layer===L.multi&&copper;}
  return false;
}


/** Shared by hit testing and document selection updates. */
export function selectableInView(item:Item,doc:{activeLayer:LayerId;layerMode?:LayerMode;view?:ViewSettings}):boolean{
 if(!onSelectableLayer(item,doc.view?.hiddenLayers??[]))return false;
 const v=doc.view;
 if(v){
  if((item.kind==='track'||item.kind==='arcTrack')&&!v.tracks)return false;
  if(item.kind==='via'&&!v.vias)return false;
  if(item.kind==='text'&&!v.text)return false;
  if(['line','arc','circle','rect','polyline','dimension'].includes(item.kind)&&!v.graphics)return false;
 }
 return (doc.layerMode!=='dim'&&doc.layerMode!=='only')||onActiveLayer(item,doc.activeLayer);
}
