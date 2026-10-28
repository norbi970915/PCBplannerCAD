import type { Bounds } from './camera';
import type { ItemId } from '../core/types';

type Leaf = { id: ItemId; bounds: Bounds };
type Node = { bounds: Bounds; parent?: Node; children?: [Node, Node]; entries?: Leaf[] };
const overlaps = (a: Bounds, b: Bounds) => a.minX <= b.maxX && a.maxX >= b.minX && a.minY <= b.maxY && a.maxY >= b.minY;
const finite = (b: Bounds) => Object.values(b).every(Number.isFinite);
const union = (bounds: Iterable<Bounds>): Bounds => {
  const out = { minX: Infinity, minY: Infinity, maxX: -Infinity, maxY: -Infinity };
  for (const b of bounds) {
    out.minX = Math.min(out.minX, b.minX); out.minY = Math.min(out.minY, b.minY);
    out.maxX = Math.max(out.maxX, b.maxX); out.maxY = Math.max(out.maxY, b.maxY);
  }
  return out;
};

/** Balanced bounding-volume tree with local refits for ordinary geometry edits. */
export class BoundsIndex {
  private root: Node | null = null;
  private leaves = new Map<ItemId, Node>();
  private refits = 0;

  rebuild(entries: Iterable<Leaf>) {
    this.leaves.clear(); this.refits = 0;
    const build = (list: Leaf[], parent?: Node): Node => {
      const bounds = union(list.map(e => e.bounds));
      const node: Node = { bounds, parent };
      if (list.length <= 12) {
        node.entries = list;
        for (const e of list) this.leaves.set(e.id, node);
      } else {
        const x = bounds.maxX - bounds.minX >= bounds.maxY - bounds.minY;
        list.sort((a,b) => x ? (a.bounds.minX+a.bounds.maxX)-(b.bounds.minX+b.bounds.maxX) : (a.bounds.minY+a.bounds.maxY)-(b.bounds.minY+b.bounds.maxY));
        const mid = list.length >> 1;
        node.children = [build(list.slice(0,mid),node), build(list.slice(mid),node)];
      }
      return node;
    };
    const valid = [...entries].filter(e => finite(e.bounds));
    this.root = valid.length ? build(valid) : null;
  }

  /** False asks the caller to rebuild for membership changes or eventual rebalance. */
  update(id: ItemId, bounds: Bounds): boolean {
    let node = this.leaves.get(id);
    if (!node || !finite(bounds) || ++this.refits > Math.max(256, this.leaves.size / 4)) return false;
    const entry = node.entries!.find(e => e.id === id)!;
    entry.bounds = bounds;
    while (node) {
      node.bounds = union(node.children ? node.children.map(c => c.bounds) : node.entries!.map(e => e.bounds));
      node = node.parent;
    }
    return true;
  }

  query(bounds: Bounds): ItemId[] {
    const ids: ItemId[] = [];
    const visit = (node: Node) => {
      if (!overlaps(node.bounds,bounds)) return;
      if (node.entries) for (const e of node.entries) if (overlaps(e.bounds,bounds)) ids.push(e.id);
      if (node.children) { visit(node.children[0]); visit(node.children[1]); }
    };
    if (this.root) visit(this.root);
    return ids;
  }
}