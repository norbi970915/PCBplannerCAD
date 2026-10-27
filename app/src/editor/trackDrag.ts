// Dragging routed tracks while they stay connected: the dragged segments move,
// connected neighbours keep their direction (45° stays 45°) and slide their shared
// end along, and ends fixed at a pad or via get a short joining segment.

import type { Footprint, Item, ItemId, Point, TrackItem } from "../core/types";
import { place } from "./draw";

type Pt = { x: number; y: number };

interface Neighbour {
  id: ItemId;
  track: TrackItem;
  /** Which end of the neighbour sits on the dragged endpoint. */
  end: "a" | "b";
}

interface End {
  track: ItemId;
  end: "a" | "b";
  at: Point;
  neighbours: Neighbour[];
  /** A pad or via holds this point in place. */
  anchored: boolean;
}

export interface TrackDragPlan {
  tracks: Map<ItemId, TrackItem>;
  ends: End[];
  /** Dragging one segment moves it perpendicular to itself only. */
  normal: Pt | null;
}

const key = (p: Point) => `${p.x},${p.y}`;
const same = (p: Point, q: Point) => p.x === q.x && p.y === q.y;

/** Copper holding a point fixed: vias and pad centres/areas on the track's layer. */
function anchorAt(items: ReadonlyMap<ItemId, Item>, footprints: Record<string, Footprint>, p: Point, layer: number): boolean {
  for (const it of items.values()) {
    if (it.kind === "via" && Math.hypot(it.pos.x - p.x, it.pos.y - p.y) <= it.diameter / 2) return true;
    if (it.kind !== "footprint") continue;
    const geom = footprints[it.footprint];
    if (!geom) continue;
    const pl = { pos: it.pos, rotation: it.rotation, mirror: it.side === "bottom" };
    for (const pad of geom.items) {
      if (pad.kind !== "pad") continue;
      const smdLayer = it.side === "bottom" ? (pad.layer === 0 ? 1 : pad.layer === 1 ? 0 : pad.layer) : pad.layer;
      if (pad.padKind === "smd" && smdLayer !== layer) continue;
      const c = place(pad.pos, pl);
      if (Math.hypot(c.x - p.x, c.y - p.y) <= Math.min(pad.width, pad.height) / 2) return true;
    }
  }
  return false;
}

/** A drag plan when every dragged item is a straight track; otherwise null (ordinary move). */
export function planTrackDrag(items: ReadonlyMap<ItemId, Item>, footprints: Record<string, Footprint>, ids: readonly ItemId[]): TrackDragPlan | null {
  const tracks = new Map<ItemId, TrackItem>();
  for (const id of ids) {
    const it = items.get(id);
    if (it?.kind !== "track") return null;
    tracks.set(id, it);
  }
  if (!tracks.size) return null;
  // Ends shared by two dragged tracks simply translate with them.
  const count = new Map<string, number>();
  for (const t of tracks.values()) for (const p of [t.a, t.b]) count.set(`${t.layer}|${key(p)}`, (count.get(`${t.layer}|${key(p)}`) ?? 0) + 1);
  const ends: End[] = [];
  for (const [id, t] of tracks) {
    for (const end of ["a", "b"] as const) {
      const at = t[end];
      if ((count.get(`${t.layer}|${key(at)}`) ?? 0) > 1) continue;
      const neighbours: Neighbour[] = [];
      for (const [nid, n] of items) {
        if (n.kind !== "track" || tracks.has(nid) || n.layer !== t.layer || n.net !== t.net) continue;
        if (same(n.a, at)) neighbours.push({ id: nid, track: n, end: "a" });
        else if (same(n.b, at)) neighbours.push({ id: nid, track: n, end: "b" });
      }
      ends.push({ track: id, end, at, neighbours, anchored: anchorAt(items, footprints, at, t.layer) });
    }
  }
  let normal: Pt | null = null;
  if (tracks.size === 1) {
    const t = [...tracks.values()][0];
    const len = Math.hypot(t.b.x - t.a.x, t.b.y - t.a.y);
    if (len > 0) normal = { x: -(t.b.y - t.a.y) / len, y: (t.b.x - t.a.x) / len };
  }
  return { tracks, ends, normal };
}

/** Intersection of the line through `p` along `d` with the line through `q` along `e`, or null if parallel. */
function intersect(p: Pt, d: Pt, q: Pt, e: Pt): Pt | null {
  const den = d.x * e.y - d.y * e.x;
  if (Math.abs(den) < 1e-9 * Math.hypot(d.x, d.y) * Math.hypot(e.x, e.y)) return null;
  const t = ((q.x - p.x) * e.y - (q.y - p.y) * e.x) / den;
  return { x: p.x + d.x * t, y: p.y + d.y * t };
}

const round = (p: Pt): Point => ({ x: Math.round(p.x), y: Math.round(p.y) });

/** Moved tracks, adjusted neighbours and new joining segments for a drag by `delta`. */
export function applyTrackDrag(plan: TrackDragPlan, delta: Pt): { replace: { id: ItemId; item: TrackItem }[]; add: TrackItem[] } {
  // One segment only moves sideways; sliding along itself changes nothing.
  const d = plan.normal ? (() => { const k = delta.x * plan.normal!.x + delta.y * plan.normal!.y; return { x: plan.normal!.x * k, y: plan.normal!.y * k }; })() : delta;
  const moved = new Map<ItemId, TrackItem>();
  for (const [id, t] of plan.tracks) moved.set(id, { ...t, a: round({ x: t.a.x + d.x, y: t.a.y + d.y }), b: round({ x: t.b.x + d.x, y: t.b.y + d.y }) });
  const neighbours = new Map<ItemId, TrackItem>();
  const add: TrackItem[] = [];
  for (const end of plan.ends) {
    const t = moved.get(end.track)!;
    const shifted = t[end.end];
    if (end.anchored) {
      // Pads and vias stay; a joining segment keeps the connection.
      if (!same(shifted, end.at)) add.push({ ...t, a: end.at, b: shifted });
      continue;
    }
    if (!end.neighbours.length) continue;
    const dir = { x: t.b.x - t.a.x, y: t.b.y - t.a.y };
    let target: Point = shifted;
    if (end.neighbours.length === 1) {
      const n = neighbours.get(end.neighbours[0].id) ?? end.neighbours[0].track;
      const far = end.neighbours[0].end === "a" ? n.b : n.a;
      const nd = { x: end.at.x - far.x, y: end.at.y - far.y };
      const x = intersect(shifted, dir, far, nd);
      // Keep the neighbour's direction unless that would fold it back over its far end.
      if (x && (x.x - far.x) * nd.x + (x.y - far.y) * nd.y > 0) target = round(x);
    }
    moved.set(end.track, { ...t, [end.end]: target });
    for (const nb of end.neighbours) {
      const n = neighbours.get(nb.id) ?? nb.track;
      neighbours.set(nb.id, { ...n, [nb.end]: target });
    }
  }
  const replace = [...moved, ...neighbours].map(([id, item]) => ({ id, item }));
  return { replace, add };
}

/** Track ends sitting on a pad of the moved footprints (same net, pad's copper layer). */
export interface Attached {
  id: ItemId;
  track: TrackItem;
  /** "both" when the segment runs between two moved pads. */
  end: "a" | "b" | "both";
  /** The single track continuing from the far end (no via there): it slides with the segment. */
  neighbour?: Neighbour;
}

export function attachedTracks(items: ReadonlyMap<ItemId, Item>, footprints: Record<string, Footprint>, ids: readonly ItemId[]): Attached[] {
  const pads: { at: Point; net: string; reach: number; layers: number[] }[] = [];
  for (const id of ids) {
    const fp = items.get(id);
    if (fp?.kind !== "footprint") continue;
    const geom = footprints[fp.footprint];
    if (!geom) continue;
    const pl = { pos: fp.pos, rotation: fp.rotation, mirror: fp.side === "bottom" };
    for (const pad of geom.items) {
      if (pad.kind !== "pad") continue;
      const net = fp.padNets[pad.number] ?? "";
      if (!net) continue;
      const smd = fp.side === "bottom" ? (pad.layer === 0 ? 1 : pad.layer === 1 ? 0 : pad.layer) : pad.layer;
      pads.push({ at: place(pad.pos, pl), net, reach: Math.max(pad.width, pad.height) / 2, layers: pad.padKind === "smd" ? [smd] : [] });
    }
  }
  const moving = new Set(ids);
  const on = (p: Point, t: TrackItem) => pads.some((pad) => pad.net === t.net && (!pad.layers.length || pad.layers.includes(t.layer)) && Math.hypot(pad.at.x - p.x, pad.at.y - p.y) <= pad.reach);
  const out: Attached[] = [];
  for (const [id, t] of items) {
    if (t.kind !== "track" || moving.has(id) || !t.net) continue;
    const a = on(t.a, t), b = on(t.b, t);
    if (!a && !b) continue;
    const entry: Attached = { id, track: t, end: a && b ? "both" : a ? "a" : "b" };
    if (entry.end !== "both") {
      const far = entry.end === "a" ? t.b : t.a;
      const via = [...items.values()].some((i) => i.kind === "via" && same(i.pos, far));
      const next: Neighbour[] = [];
      for (const [nid, n] of items) {
        if (n.kind !== "track" || nid === id || moving.has(nid) || n.layer !== t.layer || n.net !== t.net) continue;
        if (same(n.a, far)) next.push({ id: nid, track: n, end: "a" });
        else if (same(n.b, far)) next.push({ id: nid, track: n, end: "b" });
      }
      if (!via && next.length === 1) entry.neighbour = next[0];
    }
    out.push(entry);
  }
  return out;
}

/**
 * Preview of attached tracks for a footprint drag by `delta`. With `slide`, a pad segment keeps its angle and
 * its far end slides along its neighbour (mirrors the backend); otherwise, or when that is impossible, it stretches.
 */
export function stretchAttached(attached: readonly Attached[], delta: Pt, slide = false): TrackItem[] {
  const sh = (p: Point): Point => ({ x: Math.round(p.x + delta.x), y: Math.round(p.y + delta.y) });
  const out: TrackItem[] = [];
  for (const { track: t, end, neighbour } of attached) {
    if (end === "both") { out.push({ ...t, a: sh(t.a), b: sh(t.b) }); continue; }
    const near = end === "a" ? t.a : t.b, far = end === "a" ? t.b : t.a, near2 = sh(near);
    if (slide && neighbour) {
      const n = neighbour.track, other = neighbour.end === "a" ? n.b : n.a;
      const dir = { x: far.x - near.x, y: far.y - near.y }, nd = { x: far.x - other.x, y: far.y - other.y };
      const x = intersect(near2, dir, other, nd);
      if (x && (x.x - other.x) * nd.x + (x.y - other.y) * nd.y > 0 && (x.x - near2.x) * dir.x + (x.y - near2.y) * dir.y > 0) {
        const xp = round(x);
        out.push({ ...t, [end]: near2, [end === "a" ? "b" : "a"]: xp } as TrackItem, { ...n, [neighbour.end]: xp });
        continue;
      }
    }
    out.push({ ...t, [end]: near2 } as TrackItem);
  }
  return out;
}

/** Tracks a footprint drag hides while its preview is shown (attached segments and their sliding neighbours). */
export function attachedIds(attached: readonly Attached[]): ItemId[] {
  return attached.flatMap((a) => (a.neighbour ? [a.id, a.neighbour.id] : [a.id]));
}

/** Items a drag hides from the scene while its preview is shown. */
export function trackDragIds(plan: TrackDragPlan): ItemId[] {
  return [...plan.tracks.keys(), ...plan.ends.flatMap((e) => e.neighbours.map((n) => n.id))];
}
