import type { Bounds } from "./camera";
import type { ItemId } from '../core/types';
type Leaf = { id: ItemId; bounds: Bounds };
type Node = { bounds: Bounds; children?: [Node, Node]; entries?: Leaf[] };
const overlaps=(a:Bounds,b:Bounds)=>a.minX<=b.maxX&&a.maxX>=b.minX&&a.minY<=b.maxY&&a.maxY>=b.minY;
/** Balanced bounding-volume tree, rebuilt only when item bounds change. */
export class BoundsIndex {
  private root:Node|null=null;
  rebuild(entries:Iterable<Leaf>) {
    const build=(list:Leaf[]):Node=>{
      const bounds=list.reduce((b,e)=>({minX:Math.min(b.minX,e.bounds.minX),minY:Math.min(b.minY,e.bounds.minY),maxX:Math.max(b.maxX,e.bounds.maxX),maxY:Math.max(b.maxY,e.bounds.maxY)}),{minX:Infinity,minY:Infinity,maxX:-Infinity,maxY:-Infinity});
      if(list.length<=12)return {bounds,entries:list};
      const x=bounds.maxX-bounds.minX>=bounds.maxY-bounds.minY;
      list.sort((a,b)=>x?(a.bounds.minX+a.bounds.maxX)-(b.bounds.minX+b.bounds.maxX):(a.bounds.minY+a.bounds.maxY)-(b.bounds.minY+b.bounds.maxY));
      const mid=list.length>>1;return {bounds,children:[build(list.slice(0,mid)),build(list.slice(mid))]};
    };
    const valid=[...entries].filter(e=>Object.values(e.bounds).every(Number.isFinite));
    this.root=valid.length?build(valid):null;
  }
  query(bounds:Bounds):ItemId[] {
    const ids:ItemId[]=[];
    const visit=(node:Node)=>{if(!overlaps(node.bounds,bounds))return;if(node.entries)for(const e of node.entries){if(overlaps(e.bounds,bounds))ids.push(e.id);}if(node.children){visit(node.children[0]);visit(node.children[1]);}};
    if(this.root)visit(this.root);return ids;
  }
}
