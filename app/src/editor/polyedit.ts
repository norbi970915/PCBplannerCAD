// Polygon outline editing: vertex/edge handles and boundary reshaping
// (pick two points on the outline, the path between them replaces that part
// of the boundary). Pure geometry on closed point rings in nanometers.

import type { Item, ItemId, Point } from "../core/types";

export interface OutlineHit {
  /** Edge index: from vertex `edge` to vertex `edge + 1` (wrapping). */
  edge: number;
  /** Position along the edge, 0..1. */
  t: number;
  point: Point;
  dist: number;
}

/** Items whose outline can be edited, and their ring. */
export type Editable = { id: ItemId; item: Item; ring: Point[] };

export function editableRing(item: Item): Point[] | null {
  if (item.kind === "zone") return item.outline;
  if (item.kind === "polyline" && item.closed && item.points.length >= 3) return item.points;
  return null;
}

export function withRing(item: Item, ring: Point[]): Item {
  if (item.kind === "zone") return { ...item, outline: ring, fill: [] };
  if (item.kind === "polyline") return { ...item, points: ring };
  return item;
}

const sub = (a: Point, b: Point) => ({ x: a.x - b.x, y: a.y - b.y });
const same = (a: Point, b: Point) => a.x === b.x && a.y === b.y;
const round = (p: { x: number; y: number }): Point => ({ x: Math.round(p.x), y: Math.round(p.y) });

export function nearestOnRing(ring: Point[], p: Point): OutlineHit | null {
  let best: OutlineHit | null = null;
  const n = ring.length;
  for (let i = 0; i < n; i++) {
    const a = ring[i];
    const b = ring[(i + 1) % n];
    const d = sub(b, a);
    const len2 = d.x * d.x + d.y * d.y;
    const t = len2 ? Math.max(0, Math.min(1, ((p.x - a.x) * d.x + (p.y - a.y) * d.y) / len2)) : 0;
    const q = { x: a.x + d.x * t, y: a.y + d.y * t };
    const dist = Math.hypot(q.x - p.x, q.y - p.y);
    if (!best || dist < best.dist) best = { edge: i, t, point: round(q), dist };
  }
  return best;
}

/** Index of the vertex within `tol` of `p`, or -1. */
export function vertexAt(ring: Point[], p: Point, tol: number): number {
  let best = -1;
  let bd = tol;
  ring.forEach((v, i) => {
    const d = Math.hypot(v.x - p.x, v.y - p.y);
    if (d <= bd) {
      bd = d;
      best = i;
    }
  });
  return best;
}

/** Signed area (positive = counter-clockwise). */
export function signedArea(ring: Point[]): number {
  if (!ring.length) return 0;
  const origin = ring[0];
  let s = 0;
  for (let i = 0; i < ring.length; i++) {
    const a = ring[i];
    const b = ring[(i + 1) % ring.length];
    s += (a.x - origin.x) * (b.y - origin.y) - (b.x - origin.x) * (a.y - origin.y);
  }
  return s / 2;
}

/** Drops repeated and collinear vertices. */
export function cleanRing(ring: Point[]): Point[] {
  let pts = ring.filter((p, i) => !same(p, ring[(i + 1) % ring.length]));
  let changed = true;
  while (changed && pts.length > 3) {
    changed = false;
    for (let i = 0; i < pts.length; i++) {
      const a = pts[(i + pts.length - 1) % pts.length];
      const b = pts[i];
      const c = pts[(i + 1) % pts.length];
      const cross = (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
      const scale = Math.hypot(c.x - a.x, c.y - a.y) || 1;
      if (Math.abs(cross) / scale < 1 && !same(a, c) &&
          b.x >= Math.min(a.x, c.x) && b.x <= Math.max(a.x, c.x) &&
          b.y >= Math.min(a.y, c.y) && b.y <= Math.max(a.y, c.y)) {
        pts = pts.filter((_, j) => j !== i);
        changed = true;
        break;
      }
    }
  }
  return pts;
}

function segmentsCross(p1: Point, p2: Point, p3: Point, p4: Point): boolean {
  const d = (a: Point, b: Point, c: Point) => (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
  const d1 = d(p3, p4, p1);
  const d2 = d(p3, p4, p2);
  const d3 = d(p1, p2, p3);
  const d4 = d(p1, p2, p4);
  const on = (a: Point, b: Point, p: Point) => d(a, b, p) === 0 &&
    p.x >= Math.min(a.x, b.x) && p.x <= Math.max(a.x, b.x) &&
    p.y >= Math.min(a.y, b.y) && p.y <= Math.max(a.y, b.y);
  return (((d1 > 0 && d2 < 0) || (d1 < 0 && d2 > 0)) && ((d3 > 0 && d4 < 0) || (d3 < 0 && d4 > 0))) ||
    on(p1, p2, p3) || on(p1, p2, p4) || on(p3, p4, p1) || on(p3, p4, p2);
}

/** Includes touching, repeated vertices and retraced edges. */
export function selfIntersects(ring: Point[]): boolean {
  const n = ring.length;
  for (let i = 0; i < n; i++) {
    const a = ring[i];
    const b = ring[(i + 1) % n];
    const c = ring[(i + 2) % n];
    if (same(a, b)) return true;
    const ab = sub(b, a), bc = sub(c, b);
    if (ab.x * bc.y === ab.y * bc.x && ab.x * bc.x + ab.y * bc.y < 0) return true;
    for (let j = i + 2; j < n; j++) {
      if (i === 0 && j === n - 1) continue;
      if (segmentsCross(a, b, ring[j], ring[(j + 1) % n])) return true;
    }
  }
  return false;
}

export function isValidRing(ring: Point[]): boolean {
  return ring.length >= 3 && ring.every(p => Number.isSafeInteger(p.x) && Number.isSafeInteger(p.y)) && Math.abs(signedArea(ring)) > 1e6 && !selfIntersects(ring);
}

/** Boundary counts as inside. Used to choose the part under the mouse. */
export function pointInRing(ring: Point[], p: Point): boolean {
  if (nearestOnRing(ring, p)?.dist === 0) return true;
  let inside = false;
  for (let i = 0, j = ring.length - 1; i < ring.length; j = i++) {
    const a = ring[i], b = ring[j];
    if ((a.y > p.y) !== (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x) inside = !inside;
  }
  return inside;
}

export interface Reshape {
  /** The two possible results, larger area first. */
  options: [Point[], Point[]];
  valid: [boolean, boolean];
  /** Both results partition the original; a cut must not add copper. */
  partition: boolean;
}

/**
 * Replaces the boundary between `a` and `b` with `path` (which starts at
 * a.point and ends at b.point). Walking the remaining outline either way from
 * b back to a gives two closed rings: one adds or keeps the big part, the
 * other is the piece on the far side of the path.
 */
export function reshape(ring: Point[], a: OutlineHit, b: OutlineHit, path: Point[]): Reshape {
  const n = ring.length;
  // Outline positions as a continuous parameter: edge + t.
  const pa = a.edge + a.t;
  const pb = b.edge + b.t;
  // Vertices strictly between position u and position v walking forward.
  const forward = (u: number, v: number): Point[] => {
    const out: Point[] = [];
    let span = v - u;
    if (span <= 0) span += n;
    const first = Math.floor(u) + 1;
    for (let k = 0; k < n; k++) {
      const idx = first + k;
      if (idx - u >= span) break;
      out.push(ring[idx % n]);
    }
    return out;
  };
  const inner = path.slice(1, -1);
  // Option 1: a → path → b → forward along the outline → a.
  const r1 = cleanRing([a.point, ...inner, b.point, ...forward(pb, pa)]);
  // Option 2: a → path → b → backward along the outline → a.
  const back = forward(pa, pb).reverse();
  const r2 = cleanRing([a.point, ...inner, b.point, ...back]);
  const opts: [Point[], Point[]] = Math.abs(signedArea(r1)) >= Math.abs(signedArea(r2)) ? [r1, r2] : [r2, r1];
  const valid: [boolean, boolean] = [isValidRing(opts[0]), isValidRing(opts[1])];
  const area = Math.abs(signedArea(ring));
  const sum = opts.reduce((total, r) => total + Math.abs(signedArea(r)), 0);
  // Projecting onto a diagonal edge rounds the endpoint to integer nm. That
  // can move it <1 nm off the edge, changing the summed area slightly. The
  // allowance is a one-nanometer boundary strip, independent of the UI grid.
  const roundingArea = ring.reduce((total, p, i) => {
    const q = ring[(i + 1) % ring.length];
    return total + Math.hypot(q.x - p.x, q.y - p.y);
  }, 0);
  return { options: opts, valid, partition: valid.every(Boolean) && Math.abs(sum - area) <= Math.max(1, roundingArea, area * 1e-12) };
}

/** Moves edge `i` by `delta` projected on its normal (neighbors stretch). */
export function moveEdge(ring: Point[], i: number, delta: Point): Point[] {
  const n = ring.length;
  const a = ring[i];
  const b = ring[(i + 1) % n];
  const d = sub(b, a);
  const len = Math.hypot(d.x, d.y) || 1;
  const nx = -d.y / len;
  const ny = d.x / len;
  const k = delta.x * nx + delta.y * ny;
  const off = { x: nx * k, y: ny * k };
  return ring.map((p, j) => (j === i || j === (i + 1) % n ? round({ x: p.x + off.x, y: p.y + off.y }) : p));
}

export function moveVertex(ring: Point[], i: number, to: Point): Point[] {
  return ring.map((p, j) => (j === i ? to : p));
}

export function insertVertex(ring: Point[], edge: number, p: Point): Point[] {
  return [...ring.slice(0, edge + 1), p, ...ring.slice(edge + 1)];
}

export function deleteVertex(ring: Point[], i: number): Point[] {
  return ring.length > 3 ? ring.filter((_, j) => j !== i) : ring;
}

/** Snaps `p` to the nearest 45° direction from `from`. */
export function constrain45(from: Point, p: Point): Point {
  const dx = p.x - from.x;
  const dy = p.y - from.y;
  const len = Math.hypot(dx, dy);
  if (!len) return p;
  const ang = Math.round(Math.atan2(dy, dx) / (Math.PI / 4)) * (Math.PI / 4);
  const diag = Math.abs(Math.cos(ang)) > 0.1 && Math.abs(Math.sin(ang)) > 0.1;
  // Keep the snapped grid coordinate along the dominant axis.
  const l = diag ? Math.max(Math.abs(dx), Math.abs(dy)) * Math.SQRT2 : Math.abs(Math.abs(Math.cos(ang)) > 0.5 ? dx : dy);
  return round({ x: from.x + Math.cos(ang) * l, y: from.y + Math.sin(ang) * l });
}
