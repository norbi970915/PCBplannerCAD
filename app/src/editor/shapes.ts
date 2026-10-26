// Geometry helpers in world nanometers (doubles).

import earcut from "earcut";
import type { Point } from "../core/types";

export const deg2rad = (d: number) => (d * Math.PI) / 180;

export function polar(c: Point, r: number, deg: number): Point {
  const a = deg2rad(deg);
  return { x: c.x + r * Math.cos(a), y: c.y + r * Math.sin(a) };
}

export function rotatePoint(p: Point, c: Point, deg: number): Point {
  const d = ((deg % 360) + 360) % 360;
  const dx = p.x - c.x;
  const dy = p.y - c.y;
  if (d === 0) return { x: p.x, y: p.y };
  if (d === 90) return { x: c.x - dy, y: c.y + dx };
  if (d === 180) return { x: c.x - dx, y: c.y - dy };
  if (d === 270) return { x: c.x + dy, y: c.y - dx };
  const a = deg2rad(d);
  return { x: c.x + dx * Math.cos(a) - dy * Math.sin(a), y: c.y + dx * Math.sin(a) + dy * Math.cos(a) };
}

/** Polyline approximating an arc; ~5° per segment, at least 2 segments. */
export function arcPoints(c: Point, r: number, start: number, sweep: number): Point[] {
  const n = Math.max(2, Math.ceil(Math.abs(sweep) / 5));
  const pts: Point[] = [];
  for (let i = 0; i <= n; i++) pts.push(polar(c, r, start + (sweep * i) / n));
  return pts;
}

export function circlePoints(c: Point, r: number, n = 48): Point[] {
  const pts: Point[] = [];
  for (let i = 0; i < n; i++) pts.push(polar(c, r, (360 * i) / n));
  return pts;
}

/** Corners of a (rotated) rectangle centered at `c`. */
export function rectPoints(c: Point, w: number, h: number, rot: number): Point[] {
  return [
    { x: c.x - w / 2, y: c.y - h / 2 },
    { x: c.x + w / 2, y: c.y - h / 2 },
    { x: c.x + w / 2, y: c.y + h / 2 },
    { x: c.x - w / 2, y: c.y + h / 2 },
  ].map((p) => rotatePoint(p, c, rot));
}

export function roundRectPoints(c: Point, w: number, h: number, rot: number, radius: number): Point[] {
  const r = Math.min(radius, w / 2, h / 2);
  if (r <= 0) return rectPoints(c, w, h, rot);
  const corners: [number, number, number][] = [
    [w / 2 - r, h / 2 - r, 0],
    [-w / 2 + r, h / 2 - r, 90],
    [-w / 2 + r, -h / 2 + r, 180],
    [w / 2 - r, -h / 2 + r, 270],
  ];
  const pts: Point[] = [];
  for (const [x, y, a] of corners) pts.push(...arcPoints({ x: c.x + x, y: c.y + y }, r, a, 90));
  return pts.map((p) => rotatePoint(p, c, rot));
}

function area(pts: Point[]): number {
  let s = 0;
  for (let i = 0; i < pts.length; i++) {
    const a = pts[i];
    const b = pts[(i + 1) % pts.length];
    s += a.x * b.y - b.x * a.y;
  }
  return s / 2;
}

function pointInTri(p: Point, a: Point, b: Point, c: Point): boolean {
  const d1 = (p.x - b.x) * (a.y - b.y) - (a.x - b.x) * (p.y - b.y);
  const d2 = (p.x - c.x) * (b.y - c.y) - (b.x - c.x) * (p.y - c.y);
  const d3 = (p.x - a.x) * (c.y - a.y) - (c.x - a.x) * (p.y - a.y);
  const neg = d1 < 0 || d2 < 0 || d3 < 0;
  const pos = d1 > 0 || d2 > 0 || d3 > 0;
  return !(neg && pos);
}

/** Ear-clipping triangulation of a simple polygon → flat [x,y, x,y, x,y, …]. */
export function triangulate(input: Point[]): number[] {
  const pts = input.filter((p, i) => i === 0 || p.x !== input[i - 1].x || p.y !== input[i - 1].y);
  if (pts.length > 1 && pts[0].x === pts[pts.length - 1].x && pts[0].y === pts[pts.length - 1].y) pts.pop();
  if (pts.length < 3) return [];
  const poly = area(pts) < 0 ? [...pts].reverse() : pts;
  const idx = poly.map((_, i) => i);
  const out: number[] = [];
  let guard = idx.length * idx.length;
  while (idx.length > 3 && guard-- > 0) {
    let clipped = false;
    for (let i = 0; i < idx.length; i++) {
      const a = poly[idx[(i + idx.length - 1) % idx.length]];
      const b = poly[idx[i]];
      const c = poly[idx[(i + 1) % idx.length]];
      const cross = (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
      if (cross <= 0) continue;
      let inside = false;
      for (const j of idx) {
        const p = poly[j];
        if (p === a || p === b || p === c) continue;
        if (pointInTri(p, a, b, c)) {
          inside = true;
          break;
        }
      }
      if (inside) continue;
      out.push(a.x, a.y, b.x, b.y, c.x, c.y);
      idx.splice(i, 1);
      clipped = true;
      break;
    }
    if (!clipped) break;
  }
  if (idx.length === 3) {
    const [a, b, c] = idx.map((i) => poly[i]);
    out.push(a.x, a.y, b.x, b.y, c.x, c.y);
  } else if (idx.length > 3) {
    // Self-intersecting input: fall back to a fan so something still renders.
    for (let i = 1; i + 1 < idx.length; i++) {
      const [a, b, c] = [poly[idx[0]], poly[idx[i]], poly[idx[i + 1]]];
      out.push(a.x, a.y, b.x, b.y, c.x, c.y);
    }
  }
  return out;
}

/** Triangulates a polygon with holes → flat [x,y, x,y, x,y, …]. */
export function triangulateWithHoles(outer: Point[], holes: Point[][]): number[] {
  if (!holes.length) return triangulate(outer);
  const flat: number[] = [];
  const holeIdx: number[] = [];
  for (const p of outer) flat.push(p.x, p.y);
  for (const h of holes) {
    if (h.length < 3) continue;
    holeIdx.push(flat.length / 2);
    for (const p of h) flat.push(p.x, p.y);
  }
  const idx = earcut(flat, holeIdx, 2);
  const out: number[] = new Array(idx.length * 2);
  for (let i = 0; i < idx.length; i++) {
    out[i * 2] = flat[idx[i] * 2];
    out[i * 2 + 1] = flat[idx[i] * 2 + 1];
  }
  return out;
}

export function distToSegment(p: Point, a: Point, b: Point): number {
  const dx = b.x - a.x;
  const dy = b.y - a.y;
  const len2 = dx * dx + dy * dy;
  const t = len2 === 0 ? 0 : Math.max(0, Math.min(1, ((p.x - a.x) * dx + (p.y - a.y) * dy) / len2));
  return Math.hypot(p.x - (a.x + t * dx), p.y - (a.y + t * dy));
}

export function inTriangles(p: Point, tris: number[]): boolean {
  for (let i = 0; i + 5 < tris.length; i += 6) {
    if (pointInTri(p, { x: tris[i], y: tris[i + 1] }, { x: tris[i + 2], y: tris[i + 3] }, { x: tris[i + 4], y: tris[i + 5] })) return true;
  }
  return false;
}

/** Exact test: does `p` lie on the closed segment a–b (integer coordinates)? */
export function onSegment(p: Point, a: Point, b: Point): boolean {
  // |cross| / |b - a| is the distance from the line; allow sub-nanometer noise
  // (products of nm coordinates exceed double precision on long segments).
  const cross = (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x);
  return (
    Math.abs(cross) <= Math.hypot(b.x - a.x, b.y - a.y) * 0.5 &&
    p.x >= Math.min(a.x, b.x) &&
    p.x <= Math.max(a.x, b.x) &&
    p.y >= Math.min(a.y, b.y) &&
    p.y <= Math.max(a.y, b.y)
  );
}

export const samePoint = (a: Point, b: Point) => a.x === b.x && a.y === b.y;
