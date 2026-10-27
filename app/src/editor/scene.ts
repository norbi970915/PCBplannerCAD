// Per-document cache of drawn primitives plus hit testing and bounds.

import type { Item, ItemId, Point } from "../core/types";
import type { Bounds } from "./camera";
import { drawItem, type DrawContext, type Prims } from "./draw";
import type { DrawLine, DrawGroup, Rgba } from "./renderer";
import { distToSegment, inTriangles } from "./shapes";
import { mix, type Theme } from "./theme";

interface Entry {
  item: Item;
  prims: Prims;
  bounds: Bounds;
  style: number;
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
  for (const f of p.fills) {
    for (let i = 0; i < f.tris.length; i += 2) {
      minX = Math.min(minX, f.tris[i]);
      maxX = Math.max(maxX, f.tris[i]);
      minY = Math.min(minY, f.tris[i + 1]);
      maxY = Math.max(maxY, f.tris[i + 1]);
    }
  }
  return { minX, minY, maxX, maxY };
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

export class Scene {
  private entries = new Map<ItemId, Entry>();
  private ctxSignature: unknown[] = [];
  private pourFills: boolean | undefined;
  private nextStyle = 1;
  get styleCount() { return this.nextStyle; }

  /** Syncs with the document; only changed items are redrawn. */
  update(items: ReadonlyMap<ItemId, Item>, ctx: DrawContext) {
    const sig = [ctx.kind, ctx.cache, ctx.symbol, ctx.footprints, ctx.view];
    if (sig.some((v, i) => v !== this.ctxSignature[i])) {
      this.entries.clear();
      this.nextStyle = 1;
      this.ctxSignature = sig;
    }
    // Showing or hiding pour fills only changes zones.
    const fillsChanged = ctx.pourFills !== this.pourFills;
    this.pourFills = ctx.pourFills;
    for (const id of this.entries.keys()) if (!items.has(id)) this.entries.delete(id);
    for (const [id, item] of items) {
      const e = this.entries.get(id);
      if (e && e.item === item && !(fillsChanged && item.kind === "zone")) continue;
      const prims = drawItem(item, ctx);
      this.entries.set(id, { item, prims, bounds: primBounds(prims), style: e?.style ?? this.nextStyle++ });
    }
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
    for (const f of e.prims.fills) if (inTriangles(p, f.tris)) return 0;
    return best;
  }

  /** Topmost item near `p`. Parts/sheets also hit anywhere inside their outline box. */
  pick(p: Point, tol: number, accept: (item: Item) => boolean = () => true): ItemId | null {
    let best: { id: ItemId; score: number; background: boolean } | null = null;
    const probe: Bounds = { minX: p.x - tol, minY: p.y - tol, maxX: p.x + tol, maxY: p.y + tol };
    for (const [id, e] of this.entries) {
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
    for (const [id, e] of this.entries) {
      if (!accept(e.item) || !Number.isFinite(e.bounds.minX)) continue;
      if (!touching) {
        if (contains(r, e.bounds)) out.push(id);
        continue;
      }
      if (!overlaps(r, e.bounds)) continue;
      const hit =
        contains(r, e.bounds) ||
        e.prims.segs.some((s) => segHitsRect(s.a, s.b, r)) ||
        e.prims.fills.some((f) => {
          for (let i = 0; i < f.tris.length; i += 2) {
            const x = f.tris[i];
            const y = f.tris[i + 1];
            if (x >= r.minX && x <= r.maxX && y >= r.minY && y <= r.maxY) return true;
          }
          return inTriangles({ x: r.minX, y: r.minY }, f.tris);
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
    // Net highlight: everything outside the marked set fades toward the background.
    if (hl.dimOthers && hl.marked?.size) {
      const bg = theme.background;
      for (const [id, e] of this.entries) if (!hl.marked.has(id) && !hl.excluded?.has(id)) out.set(e.style, [bg[0], bg[1], bg[2], 0.75]);
    }
    if (hl.hover !== null) add(hl.hover, 0.3, [1, 1, 1, 1]);
    for (const id of hl.marked ?? []) add(id, 0.4, theme.selectTint);
    for (const id of hl.selection) add(id, 0.55, theme.selectTint);
    return out;
  }

  /** Builds render groups ordered by theme rank. */
  build(theme: Theme, hl: HighlightState): DrawGroup[] {
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
