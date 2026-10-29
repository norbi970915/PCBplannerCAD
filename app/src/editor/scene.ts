import { pointInRing } from "./polyedit";
import { BoundsIndex } from "./boundsIndex";
// Per-document cache of drawn primitives plus hit testing and bounds.

import type { Item, ItemId, Point, ZonePolygon } from "../core/types";
import type { Bounds } from "./camera";
import { drawItem, type DrawContext, type Prims } from "./draw";
import type { DrawLine, DrawGroup, Rgba } from "./renderer";
import { distToSegment, inTriangles, onSegment } from "./shapes";
import { mix, type Theme } from "./theme";

interface Entry {
  item: Item;
  prims: Prims;
  bounds: Bounds;
  style: number;
}

const fillGeometryCache = new WeakMap<ZonePolygon, { bounds: Bounds; holes: { ring: Point[]; bounds: Bounds }[] }>();
function ringBounds(ring: Point[]): Bounds {
  const bounds = { minX: Infinity, minY: Infinity, maxX: -Infinity, maxY: -Infinity };
  for (const p of ring) { bounds.minX = Math.min(bounds.minX, p.x); bounds.minY = Math.min(bounds.minY, p.y); bounds.maxX = Math.max(bounds.maxX, p.x); bounds.maxY = Math.max(bounds.maxY, p.y); }
  return bounds;
}
function fillGeometry(polygon: ZonePolygon) {
  let geometry = fillGeometryCache.get(polygon);
  if (!geometry) fillGeometryCache.set(polygon, geometry = { bounds: ringBounds(polygon.outer), holes: polygon.holes.map(ring => ({ ring, bounds: ringBounds(ring) })) });
  return geometry;
}
const containsPoint = (bounds: Bounds, p: Point) => p.x >= bounds.minX && p.x <= bounds.maxX && p.y >= bounds.minY && p.y <= bounds.maxY;
function fillContains(polygon: ZonePolygon, p: Point): boolean {
  const geometry = fillGeometry(polygon);
  if (!containsPoint(geometry.bounds, p) || !pointInRing(polygon.outer, p)) return false;
  for (const hole of geometry.holes) if (containsPoint(hole.bounds, p) && pointInRing(hole.ring, p)) {
    if (!hole.ring.some((a, i) => onSegment(p, a, hole.ring[(i + 1) % hole.ring.length]))) return false;
  }
  return true;
}

function primBounds(p: Prims): Bounds {
  let minX = Infinity;
  let minY = Infinity;
  let maxX = -Infinity;
  let maxY = -Infinity;
  for (const s of p.segs) {
    const h = s.w / 2;
    minX = Math.min(minX, s.a.x - h, s.b.x - h);
    minY = Math.min(minY, s.a.y - h, s.b.y - h);
    maxX = Math.max(maxX, s.a.x + h, s.b.x + h);
    maxY = Math.max(maxY, s.a.y + h, s.b.y + h);
  }
  for (const f of p.fillPolygons ? [] : p.fills) {
    for (let i = 0; i < f.tris.length; i += 2) {
      minX = Math.min(minX, f.tris[i]);
      maxX = Math.max(maxX, f.tris[i]);
      minY = Math.min(minY, f.tris[i + 1]);
      maxY = Math.max(maxY, f.tris[i + 1]);
    }
  }
  for (const polygon of p.fillPolygons ?? []) {
    const bounds = fillGeometry(polygon).bounds;
    minX = Math.min(minX, bounds.minX); minY = Math.min(minY, bounds.minY);
    maxX = Math.max(maxX, bounds.maxX); maxY = Math.max(maxY, bounds.maxY);
  }
  return { minX, minY, maxX, maxY };
}

/** Round-capped stroke/box contact, including circular pads and vias. */
function strokeHitsRect(a:Point,b:Point,width:number,r:Bounds):boolean {
 if(segHitsRect(a,b,r))return true;
 const gap=(p:Point)=>Math.hypot(Math.max(r.minX-p.x,0,p.x-r.maxX),Math.max(r.minY-p.y,0,p.y-r.maxY));
 const corners=[{x:r.minX,y:r.minY},{x:r.maxX,y:r.minY},{x:r.maxX,y:r.maxY},{x:r.minX,y:r.maxY}];
 return Math.min(gap(a),gap(b),...corners.map(p=>distToSegment(p,a,b)))<=width/2;
}

const overlaps = (a: Bounds, b: Bounds) => a.minX <= b.maxX && a.maxX >= b.minX && a.minY <= b.maxY && a.maxY >= b.minY;
const contains = (outer: Bounds, inner: Bounds) =>
  inner.minX >= outer.minX && inner.maxX <= outer.maxX && inner.minY >= outer.minY && inner.maxY <= outer.maxY;

function segHitsRect(a: Point, b: Point, r: Bounds): boolean {
  // Liang–Barsky clip.
  let t0 = 0;
  let t1 = 1;
  const dx = b.x - a.x;
  const dy = b.y - a.y;
  const clip = (p: number, q: number) => {
    if (p === 0) return q >= 0;
    const t = q / p;
    if (p < 0) {
      if (t > t1) return false;
      if (t > t0) t0 = t;
    } else {
      if (t < t0) return false;
      if (t < t1) t1 = t;
    }
    return true;
  };
  return clip(-dx, a.x - r.minX) && clip(dx, r.maxX - a.x) && clip(-dy, a.y - r.minY) && clip(dy, r.maxY - a.y);
}

export interface HighlightState {
  selection: ReadonlySet<ItemId>;
  hover: ItemId | null;
  excluded?: ReadonlySet<ItemId>;
  /** Extra items highlighted (net highlight, ERC targets). */
  marked?: ReadonlySet<ItemId>;
  /** Fade everything that is not marked (board net highlight). */
  dimOthers?: boolean;
}

/** Board-wide fading is a shader uniform, leaving only explicit highlights in the table. */
export function highlightDimming(theme: Theme, hl: HighlightState): Rgba {
  return hl.dimOthers && hl.marked?.size ? [theme.background[0], theme.background[1], theme.background[2], 0.75] : [0, 0, 0, 0];
}

export class Scene {
  private entries = new Map<ItemId, Entry>();
  private index = new BoundsIndex();
  private ctxSignature: unknown[] = [];
  private pourFills: boolean | undefined;
  private copperLayer: number | undefined;
  private nextStyle = 1;
  private compiled = new WeakMap<Entry, { theme: Theme; groups: DrawGroup[] }>();
  private built = new Map<number, { sources: DrawGroup[]; group: DrawGroup }>();
  get styleCount() { return this.nextStyle; }

  /** Syncs with the document; only changed items are redrawn. */
  update(items: ReadonlyMap<ItemId, Item>, ctx: DrawContext) {
    let rebuildIndex = false;
    const moved: { id: ItemId; bounds: Bounds }[] = [];
    const sig = [ctx.kind, ctx.cache, ctx.symbol, ctx.footprints, ctx.view, ctx.smoothCopper];
    if (sig.some((v, i) => v !== this.ctxSignature[i])) {
      rebuildIndex = true;
      this.entries.clear();
      this.nextStyle = 1;
      this.ctxSignature = sig;
    }
    const layerChanged = ctx.copperLayer !== this.copperLayer;
    this.copperLayer = ctx.copperLayer;
    const layerDependent = (item: Item) => item.kind === "pad" ? !!item.copperShapes && Object.keys(item.copperShapes).length > 0
      : item.kind === "footprint" && !!ctx.footprints?.[item.footprint]?.items.some(pad => pad.kind === "pad" && !!pad.copperShapes && Object.keys(pad.copperShapes).length > 0);
    // Showing or hiding pour fills only changes zones.
    const fillsChanged = ctx.pourFills !== this.pourFills;
    this.pourFills = ctx.pourFills;
    for (const id of this.entries.keys()) if (!items.has(id)) { this.entries.delete(id); rebuildIndex = true; }
    for (const [id, item] of items) {
      const e = this.entries.get(id);
      if (e && e.item === item && !e.prims.pendingFills?.length && !(fillsChanged && item.kind === "zone") && !(layerChanged && layerDependent(item))) continue;
      const prims = drawItem(item, ctx);
      const bounds = primBounds(prims);
      this.entries.set(id, { item, prims, bounds, style: e?.style ?? this.nextStyle++ });
      if (!e) rebuildIndex = true;
      else if (bounds.minX !== e.bounds.minX || bounds.minY !== e.bounds.minY || bounds.maxX !== e.bounds.maxX || bounds.maxY !== e.bounds.maxY) moved.push({ id, bounds });
    }
    if (!rebuildIndex) for (const entry of moved) if (!this.index.update(entry.id, entry.bounds)) { rebuildIndex = true; break; }
    if (rebuildIndex) this.index.rebuild([...this.entries].map(([id,e])=>({id,bounds:e.bounds})));
  }

  prims(id: ItemId): Prims | undefined {
    return this.entries.get(id)?.prims;
  }

  bounds(ids?: Iterable<ItemId>): Bounds | null {
    let out: Bounds | null = null;
    const list = ids ? [...ids].map((id) => this.entries.get(id)).filter((e): e is Entry => !!e) : this.entries.values();
    for (const e of list) {
      if (!Number.isFinite(e.bounds.minX)) continue;
      out = out
        ? {
            minX: Math.min(out.minX, e.bounds.minX),
            minY: Math.min(out.minY, e.bounds.minY),
            maxX: Math.max(out.maxX, e.bounds.maxX),
            maxY: Math.max(out.maxY, e.bounds.maxY),
          }
        : { ...e.bounds };
    }
    return out;
  }

  /** Distance from `p` to the item's drawn geometry (0 when inside a fill). */
  distance(id: ItemId, p: Point): number {
    const e = this.entries.get(id);
    if (!e) return Infinity;
    let best = Infinity;
    for (const s of e.prims.segs) best = Math.min(best, Math.max(0, distToSegment(p, s.a, s.b) - s.w / 2));
    if (e.prims.fillPolygons) {
      if (e.prims.fillPolygons.some(polygon => fillContains(polygon, p))) return 0;
    } else for (const f of e.prims.fills) if (inTriangles(p, f.tris)) return 0;
    return best;
  }

  /** Topmost item near `p`. Parts/sheets also hit anywhere inside their outline box. */
  pick(p: Point, tol: number, accept: (item: Item) => boolean = () => true): ItemId | null {
    let best: { id: ItemId; score: number; background: boolean } | null = null;
    const probe: Bounds = { minX: p.x - tol, minY: p.y - tol, maxX: p.x + tol, maxY: p.y + tol };
    for (const id of this.index.query(probe).sort((a,b)=>this.entries.get(a)!.style-this.entries.get(b)!.style)) {
      const e = this.entries.get(id)!;
      if (!overlaps(e.bounds, probe) || !accept(e.item)) continue;
      let score = this.distance(id, p);
      if (score > tol) {
        const boxy = e.item.kind === "part" || e.item.kind === "sheet" || e.item.kind === "footprint";
        if (boxy && contains(e.bounds, { minX: p.x, minY: p.y, maxX: p.x, maxY: p.y })) score = tol * 0.9;
        else continue;
      }
      // Small things win over big ones at equal distance.
      const size = (e.bounds.maxX - e.bounds.minX) * (e.bounds.maxY - e.bounds.minY);
      const s = score + Math.min(tol * 0.1, Math.sqrt(size) * 1e-4);
      const background = e.item.kind === "zone";
      if (!best || (best.background && !background) || (best.background === background && s <= best.score)) best = { id, score: s, background };
    }
    return best?.id ?? null;
  }

  inRect(r: Bounds, touching: boolean, accept: (item: Item) => boolean = () => true): ItemId[] {
    const out: ItemId[] = [];
    for (const id of this.index.query(r).sort((a,b)=>this.entries.get(a)!.style-this.entries.get(b)!.style)) {
      const e = this.entries.get(id)!;
      if (!accept(e.item) || !Number.isFinite(e.bounds.minX)) continue;
      if (!touching) {
        if (contains(r, e.bounds)) out.push(id);
        continue;
      }
      if (!overlaps(r, e.bounds)) continue;
      const hit =
        contains(r, e.bounds) ||
        e.prims.segs.some((s) => strokeHitsRect(s.a, s.b, s.w, r)) ||
        (!e.prims.fillPolygons && e.prims.fills.some((f) => {
          for (let i = 0; i < f.tris.length; i += 2) {
            const x = f.tris[i];
            const y = f.tris[i + 1];
            if (x >= r.minX && x <= r.maxX && y >= r.minY && y <= r.maxY) return true;
          }
          return inTriangles({ x: r.minX, y: r.minY }, f.tris);
        })) || (e.prims.fillPolygons ?? []).some(polygon => {
          const corners = [{ x: r.minX, y: r.minY }, { x: r.maxX, y: r.minY }, { x: r.maxX, y: r.maxY }, { x: r.minX, y: r.maxY }];
          if (corners.some(p => fillContains(polygon, p)) || polygon.outer.some(p => containsPoint(r, p))) return true;
          const boundaryHits = (ring: Point[]) => ring.some((p, i) => segHitsRect(p, ring[(i + 1) % ring.length], r));
          return boundaryHits(polygon.outer) || fillGeometry(polygon).holes.some(h => overlaps(h.bounds, r) && boundaryHits(h.ring));
        });
      if (hit) out.push(id);
    }
    return out;
  }

  /** Sparse highlight changes leave the board vertex buffers untouched. */
  highlights(theme: Theme, hl: HighlightState): Map<number, Rgba> {
    const out = new Map<number, Rgba>();
    const add = (id: ItemId, amount: number, tint: Rgba) => {
      const e = this.entries.get(id);
      if (e && !hl.excluded?.has(id)) out.set(e.style, [tint[0], tint[1], tint[2], amount]);
    };
    if (hl.hover !== null) add(hl.hover, 0.3, [1, 1, 1, 1]);
    for (const id of hl.marked ?? []) add(id, 0.4, theme.selectTint);
    for (const id of hl.selection) add(id, 0.55, theme.selectTint);
    return out;
  }

  /** Builds render groups ordered by theme rank. */
  build(theme: Theme, hl: HighlightState): DrawGroup[] {
    // Interactive highlights use a texture; the scene itself normally has no tint.
    if (!hl.selection.size && hl.hover === null && !hl.marked?.size) return this.buildCached(theme, hl.excluded);
    const groups = new Map<number, DrawGroup>();
    const group = (rank: number) => {
      let g = groups.get(rank);
      if (!g) groups.set(rank, (g = { rank, lines: [], fills: [] }));
      return g;
    };
    for (const [id, e] of this.entries) {
      if (hl.excluded?.has(id)) continue;
      const k = hl.selection.has(id) ? 0.55 : hl.marked?.has(id) ? 0.4 : id === hl.hover ? 0.3 : 0;
      const tint = hl.selection.has(id) || hl.marked?.has(id) ? theme.selectTint : ([1, 1, 1, 1] as Rgba);
      pushPrims(e.prims, theme, group, k, tint, e.style);
    }
    return [...groups.values()].sort((a, b) => a.rank - b.rank);
  }

  /** Reuses unchanged layer batches and their fill/line arrays independently. */
  private buildCached(theme: Theme, excluded?: ReadonlySet<ItemId>): DrawGroup[] {
    const sources = new Map<number, DrawGroup[]>();
    for (const [id, entry] of this.entries) {
      if (excluded?.has(id)) continue;
      let cached = this.compiled.get(entry);
      if (!cached || cached.theme !== theme) {
        const groups = new Map<number, DrawGroup>();
        pushPrims(entry.prims, theme, rank => {
          let group = groups.get(rank);
          if (!group) groups.set(rank, group = { rank, lines: [], fills: [] });
          return group;
        }, 0, theme.selectTint, entry.style);
        cached = { theme, groups: [...groups.values()] };
        this.compiled.set(entry, cached);
      }
      for (const group of cached.groups) {
        let list = sources.get(group.rank);
        if (!list) sources.set(group.rank, list = []);
        list.push(group);
      }
    }
    const built = new Map<number, { sources: DrawGroup[]; group: DrawGroup }>();
    const same = <T,>(a: readonly T[], b: readonly T[]) => a.length === b.length && a.every((value, i) => value === b[i]);
    for (const [rank, list] of sources) {
      const previous = this.built.get(rank);
      if (previous && same(previous.sources, list)) { built.set(rank, previous); continue; }
      const group: DrawGroup = { rank, lines: [], fills: [] };
      for (const source of list) {
        for (const line of source.lines) group.lines.push(line);
        for (const fill of source.fills) group.fills.push(fill);
      }
      // Editing a track must not re-upload the unchanged pours on that layer.
      if (previous && same(previous.group.lines, group.lines)) group.lines = previous.group.lines;
      if (previous && same(previous.group.fills, group.fills)) group.fills = previous.group.fills;
      built.set(rank, { sources: list, group });
    }
    this.built = built;
    return [...built.values()].map(b => b.group).sort((a, b) => a.rank - b.rank);
  }

}

export function pushPrims(prims: Prims, theme: Theme, group: (rank: number) => DrawGroup, k: number, tint: Rgba, style = 0) {
  const color = (role: string) => {
    const c = theme.color(role);
    return k ? mix(c, tint, k) : c;
  };
  for (const s of prims.segs) {
    const line: DrawLine = { a: s.a, b: s.b, width: s.w, color: color(s.role), style };
    group(theme.rank(s.role)).lines.push(line);
  }
  // Fills (symbol bodies, pads) get a much lighter tint so highlighted
  // objects stay readable instead of washing out.
  const fillColor = (role: string) => {
    const c = theme.color(role);
    return k ? mix(c, tint, k * 0.3) : c;
  };
  for (const f of prims.fills) group(theme.rank(f.role)).fills.push({ tris: f.tris, color: fillColor(f.role), style });
}

/** Draws free-standing items (tool previews) into a single overlay group list. */
export function buildPreview(items: Item[], ctx: DrawContext, theme: Theme, k = 0.35): DrawGroup[] {
  const groups = new Map<number, DrawGroup>();
  const group = (rank: number) => {
    let g = groups.get(rank);
    if (!g) groups.set(rank, (g = { rank, lines: [], fills: [] }));
    return g;
  };
  for (const item of items) pushPrims(drawItem(item, ctx), theme, group, k, theme.selectTint);
  return [...groups.values()].sort((a, b) => a.rank - b.rank);
}
