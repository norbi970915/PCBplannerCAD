// Board editing helpers without UI: Inspect ▸ Board Statistics (with the drill table),
// Align / Distribute footprints, Select Connection (U) and Unroute.

import type { AirWire, Footprint, IdItem, Item, ItemId, LayerId, PlacedFootprintItem, Point } from "../core/types";
import { L } from "../core/layers";
import { place } from "../editor/draw";
import { rectPoints } from "../editor/shapes";
import { viaKind, type ViaKind } from "./vias";

type Items = ReadonlyMap<ItemId, Item>;
type Footprints = Record<string, Footprint>;
export interface Box { minX: number; minY: number; maxX: number; maxY: number }

const key = (p: Point) => `${p.x},${p.y}`;
const flipLayer = (l: LayerId) => (l === L.topCopper ? L.bottomCopper : l === L.bottomCopper ? L.topCopper : l);

/** Copper layers of the board in stack order (top first). */
export function copperLayers(items: Items): LayerId[] {
  for (const it of items.values()) {
    if (it.kind === "board") return it.stackup.layers.filter((l) => l.kind === "copper" && l.layer !== null).map((l) => l.layer as LayerId);
  }
  return [L.topCopper, L.bottomCopper];
}

interface WorldPad { owner: ItemId; number: string; net: string; at: Point; w: number; h: number; rot: number; smd: boolean; layer: LayerId; padKind: "smd" | "tht" | "npth"; drill: { width: number; height: number } | null }

function worldPads(fp: PlacedFootprintItem, owner: ItemId, footprints: Footprints): WorldPad[] {
  const geom = footprints[fp.footprint];
  if (!geom) return [];
  const pl = { pos: fp.pos, rotation: fp.rotation, mirror: fp.side === "bottom" };
  const out: WorldPad[] = [];
  for (const pad of geom.items) {
    if (pad.kind !== "pad") continue;
    out.push({
      owner,
      number: pad.number,
      net: fp.padNets[pad.number] ?? "",
      at: place(pad.pos, pl),
      w: pad.width,
      h: pad.height,
      rot: fp.rotation + (pl.mirror ? -pad.rotation : pad.rotation),
      smd: pad.padKind === "smd",
      layer: fp.side === "bottom" ? flipLayer(pad.layer) : pad.layer,
      padKind: pad.padKind,
      drill: pad.drill,
    });
  }
  return out;
}

// ---------- Board statistics ----------

export interface DrillRow { kind: "via" | "pad" | "slot"; plated: boolean; width: number; height: number; count: number; label: string }

export interface BoardStats {
  footprints: { total: number; top: number; bottom: number; smd: number; tht: number; mixed: number; locked: number };
  pads: { smd: number; tht: number; npth: number };
  vias: Record<ViaKind, number>;
  tracks: { segments: number; arcs: number; length: number; byLayer: Map<LayerId, number> };
  zones: { pours: number; keepouts: number; teardrops: number };
  nets: number;
  unrouted: number;
  copperLayers: number;
  /** Board outline extent, or null without an outline. */
  size: { width: number; height: number } | null;
  drills: DrillRow[];
}

function outlineBox(items: Items): Box | null {
  let b: Box | null = null;
  const add = (p: Point, r = 0) => {
    b = b ?? { minX: Infinity, minY: Infinity, maxX: -Infinity, maxY: -Infinity };
    b.minX = Math.min(b.minX, p.x - r);
    b.minY = Math.min(b.minY, p.y - r);
    b.maxX = Math.max(b.maxX, p.x + r);
    b.maxY = Math.max(b.maxY, p.y + r);
  };
  for (const it of items.values()) {
    if (!("layer" in it) || it.layer !== L.boardOutline) continue;
    if (it.kind === "line" || it.kind === "rect") [it.a, it.b].forEach((p) => add(p));
    else if (it.kind === "circle" || it.kind === "arc") add(it.center, it.radius);
    else if (it.kind === "polyline") it.points.forEach((p) => add(p));
  }
  return b;
}

export function boardStats(items: Items, footprints: Footprints, airwires: readonly AirWire[]): BoardStats {
  const copper = copperLayers(items);
  const s: BoardStats = {
    footprints: { total: 0, top: 0, bottom: 0, smd: 0, tht: 0, mixed: 0, locked: 0 },
    pads: { smd: 0, tht: 0, npth: 0 },
    vias: { through: 0, blind: 0, buried: 0, micro: 0 },
    tracks: { segments: 0, arcs: 0, length: 0, byLayer: new Map() },
    zones: { pours: 0, keepouts: 0, teardrops: 0 },
    nets: 0,
    unrouted: airwires.length,
    copperLayers: copper.length,
    size: null,
    drills: [],
  };
  const nets = new Set<string>();
  const drills = new Map<string, DrillRow>();
  const drill = (kind: DrillRow["kind"], plated: boolean, w: number, h: number) => {
    const k = `${kind}|${plated}|${w}|${h}`;
    const row = drills.get(k) ?? { kind, plated, width: w, height: h, count: 0, label: "" };
    row.count++;
    drills.set(k, row);
  };
  const length = (layer: LayerId, len: number) => {
    s.tracks.length += len;
    s.tracks.byLayer.set(layer, (s.tracks.byLayer.get(layer) ?? 0) + len);
  };
  for (const [id, it] of items) {
    switch (it.kind) {
      case "footprint": {
        const f = s.footprints;
        f.total++;
        it.side === "bottom" ? f.bottom++ : f.top++;
        if (it.locked) f.locked++;
        const pads = worldPads(it, id, footprints);
        const smd = pads.some((p) => p.padKind === "smd"), tht = pads.some((p) => p.padKind !== "smd");
        if (smd && tht) f.mixed++;
        else if (tht) f.tht++;
        else if (smd) f.smd++;
        for (const p of pads) {
          s.pads[p.padKind]++;
          if (p.net) nets.add(p.net);
          if (p.drill) drill(p.drill.width === p.drill.height ? "pad" : "slot", p.padKind === "tht", Math.min(p.drill.width, p.drill.height), Math.max(p.drill.width, p.drill.height));
        }
        break;
      }
      case "via":
        s.vias[viaKind(copper, it.fromLayer, it.toLayer, it.drill)]++;
        drill("via", true, it.drill, it.drill);
        break;
      case "track":
        s.tracks.segments++;
        length(it.layer, Math.hypot(it.b.x - it.a.x, it.b.y - it.a.y));
        break;
      case "arcTrack":
        s.tracks.arcs++;
        length(it.layer, (Math.abs(it.sweep) * Math.PI * it.radius) / 180);
        break;
      case "zone":
        if (it.keepout) s.zones.keepouts++;
        else if (it.teardrop) s.zones.teardrops++;
        else s.zones.pours++;
        break;
    }
  }
  s.nets = nets.size;
  const box = outlineBox(items);
  if (box) s.size = { width: box.maxX - box.minX, height: box.maxY - box.minY };
  // Drill table: plated before non-plated, then by size; T1, T2… like an Excellon tool list.
  s.drills = [...drills.values()].sort((a, b) => Number(b.plated) - Number(a.plated) || a.width - b.width || a.height - b.height || a.kind.localeCompare(b.kind));
  s.drills.forEach((r, i) => (r.label = `T${i + 1}`));
  return s;
}

// ---------- Align / distribute ----------

export type AlignMode = "left" | "right" | "top" | "bottom" | "centerX" | "centerY";

/** World extent of a footprint: its pads and graphics, or its origin when it has neither. */
export function footprintBox(fp: PlacedFootprintItem, footprints: Footprints): Box {
  const b: Box = { minX: Infinity, minY: Infinity, maxX: -Infinity, maxY: -Infinity };
  const add = (p: Point) => {
    b.minX = Math.min(b.minX, p.x);
    b.minY = Math.min(b.minY, p.y);
    b.maxX = Math.max(b.maxX, p.x);
    b.maxY = Math.max(b.maxY, p.y);
  };
  const pl = { pos: fp.pos, rotation: fp.rotation, mirror: fp.side === "bottom" };
  for (const it of footprints[fp.footprint]?.items ?? []) {
    if (it.kind === "pad") rectPoints(place(it.pos, pl), it.width, it.height, fp.rotation + (pl.mirror ? -it.rotation : it.rotation)).forEach(add);
    else if (it.kind === "line" || it.kind === "rect") [it.a, it.b].forEach((p) => add(place(p, pl)));
    else if (it.kind === "circle") [-1, 1].forEach((k) => add(place({ x: it.center.x + k * it.radius, y: it.center.y + k * it.radius }, pl)));
    else if (it.kind === "polyline") it.points.forEach((p) => add(place(p, pl)));
  }
  if (b.minX === Infinity) add(fp.pos);
  return b;
}

const center = (b: Box) => ({ x: (b.minX + b.maxX) / 2, y: (b.minY + b.maxY) / 2 });

function movable(items: Items, footprints: Footprints, ids: Iterable<ItemId>) {
  const out: { id: ItemId; fp: PlacedFootprintItem; box: Box }[] = [];
  for (const id of ids) {
    const it = items.get(id);
    if (it?.kind === "footprint" && !it.locked) out.push({ id, fp: it, box: footprintBox(it, footprints) });
  }
  return out;
}

const moved = (id: ItemId, fp: PlacedFootprintItem, dx: number, dy: number): IdItem => ({ id, item: { ...fp, pos: { x: Math.round(fp.pos.x + dx), y: Math.round(fp.pos.y + dy) } } });

/** Lines up the selected (unlocked) footprints' extents on the selection's outermost edge or centre line. */
export function alignFootprints(items: Items, footprints: Footprints, ids: Iterable<ItemId>, mode: AlignMode): IdItem[] {
  const parts = movable(items, footprints, ids);
  if (parts.length < 2) return [];
  const all = parts.reduce((b, p) => ({ minX: Math.min(b.minX, p.box.minX), minY: Math.min(b.minY, p.box.minY), maxX: Math.max(b.maxX, p.box.maxX), maxY: Math.max(b.maxY, p.box.maxY) }), parts[0].box);
  const c = center(all);
  const out: IdItem[] = [];
  for (const { id, fp, box } of parts) {
    const d = {
      left: [all.minX - box.minX, 0],
      right: [all.maxX - box.maxX, 0],
      // Y points up: top is the largest y.
      top: [0, all.maxY - box.maxY],
      bottom: [0, all.minY - box.minY],
      centerX: [c.x - center(box).x, 0],
      centerY: [0, c.y - center(box).y],
    }[mode];
    if (Math.round(d[0]) || Math.round(d[1])) out.push(moved(id, fp, d[0], d[1]));
  }
  return out;
}

/** Equal gaps between the selected footprints along `axis`; the two outermost stay put. */
export function distributeFootprints(items: Items, footprints: Footprints, ids: Iterable<ItemId>, axis: "x" | "y"): IdItem[] {
  const parts = movable(items, footprints, ids);
  if (parts.length < 3) return [];
  const lo = (b: Box) => (axis === "x" ? b.minX : b.minY), hi = (b: Box) => (axis === "x" ? b.maxX : b.maxY);
  parts.sort((a, b) => lo(a.box) + hi(a.box) - (lo(b.box) + hi(b.box)));
  const span = hi(parts[parts.length - 1].box) - lo(parts[0].box);
  const gap = (span - parts.reduce((s, p) => s + hi(p.box) - lo(p.box), 0)) / (parts.length - 1);
  const out: IdItem[] = [];
  let at = hi(parts[0].box) + gap;
  for (const { id, fp, box } of parts.slice(1, -1)) {
    const d = at - lo(box);
    if (Math.round(d)) out.push(moved(id, fp, axis === "x" ? d : 0, axis === "y" ? d : 0));
    at += hi(box) - lo(box) + gap;
  }
  return out;
}

/**
 * Pack & Move: the selected footprints in tight rows (tallest first), starting at the selection's top-left.
 * Moves are whole multiples of `grid`, so parts on the grid stay on it.
 */
export function packFootprints(items: Items, footprints: Footprints, ids: Iterable<ItemId>, gap: number, grid = 1): IdItem[] {
  const parts = movable(items, footprints, ids);
  if (!parts.length) return [];
  const w = (b: Box) => b.maxX - b.minX, h = (b: Box) => b.maxY - b.minY;
  const left = Math.min(...parts.map((p) => p.box.minX)), top = Math.max(...parts.map((p) => p.box.maxY));
  const area = parts.reduce((a, p) => a + (w(p.box) + gap) * (h(p.box) + gap), 0);
  const rowWidth = Math.max(Math.sqrt(area) * 1.2, ...parts.map((p) => w(p.box)));
  parts.sort((a, b) => h(b.box) - h(a.box) || w(b.box) - w(a.box) || (a.fp.designator < b.fp.designator ? -1 : 1));
  const snap = (v: number) => Math.round(v / grid) * grid;
  const out: IdItem[] = [];
  let x = left, y = top, rowH = 0;
  for (const { id, fp, box } of parts) {
    if (x > left && x + w(box) > left + rowWidth) {
      y -= rowH + gap;
      x = left;
      rowH = 0;
    }
    const dx = snap(x - box.minX), dy = snap(y - box.maxY);
    if (dx || dy) out.push(moved(id, fp, dx, dy));
    x += w(box) + gap;
    rowH = Math.max(rowH, h(box));
  }
  return out;
}

// ---------- Select connection (U) / unroute ----------

/**
 * How far a copper walk goes:
 * - `chain`: the segments between pads, vias and junctions (the first U press);
 * - `connection`: everything joined by tracks and vias up to the pads (second U);
 * - `unroute`: like `chain` but through vias, which go along unless shared with other routing or a pour.
 */
export type WalkScope = "chain" | "connection" | "unroute";

type Wire = { id: ItemId; layer: LayerId; net: string; ends: [Point, Point] };

function wires(items: Items): Wire[] {
  const out: Wire[] = [];
  for (const [id, it] of items) {
    if (it.kind === "track") out.push({ id, layer: it.layer, net: it.net, ends: [it.a, it.b] });
    else if (it.kind === "arcTrack") {
      const at = (deg: number) => ({ x: Math.round(it.center.x + it.radius * Math.cos((deg * Math.PI) / 180)), y: Math.round(it.center.y + it.radius * Math.sin((deg * Math.PI) / 180)) });
      out.push({ id, layer: it.layer, net: it.net, ends: [at(it.startAngle), at(it.startAngle + it.sweep)] });
    }
  }
  return out;
}

function inPolygon(p: Point, ring: readonly Point[]): boolean {
  let inside = false;
  for (let i = 0, j = ring.length - 1; i < ring.length; j = i++) {
    const a = ring[i], b = ring[j];
    if (a.y > p.y !== b.y > p.y && p.x < a.x + ((p.y - a.y) * (b.x - a.x)) / (b.y - a.y)) inside = !inside;
  }
  return inside;
}

/** Tracks (and, past `chain`, vias) reached from `start` within `scope`. */
export function walkCopper(items: Items, footprints: Footprints, start: Iterable<ItemId>, scope: WalkScope): Set<ItemId> {
  const copper = copperLayers(items);
  const all = wires(items);
  const byEnd = new Map<string, Wire[]>();
  for (const w of all) for (const p of w.ends) {
    const k = `${w.layer}|${key(p)}`;
    byEnd.set(k, [...(byEnd.get(k) ?? []), w]);
  }
  const pads: WorldPad[] = [];
  const vias: { id: ItemId; pos: Point; r: number; net: string; layers: LayerId[] }[] = [];
  const pours: { net: string; layer: LayerId; outline: Point[] }[] = [];
  for (const [id, it] of items) {
    if (it.kind === "footprint") pads.push(...worldPads(it, id, footprints));
    else if (it.kind === "via") {
      const i = copper.indexOf(it.fromLayer), j = copper.indexOf(it.toLayer);
      vias.push({ id, pos: it.pos, r: it.diameter / 2, net: it.net, layers: i < 0 || j < 0 ? [it.fromLayer, it.toLayer] : copper.slice(Math.min(i, j), Math.max(i, j) + 1) });
    } else if (it.kind === "zone" && !it.keepout && !it.teardrop) pours.push({ net: it.net, layer: it.layer, outline: it.outline });
  }
  const padAt = (p: Point, layer: LayerId) => pads.some((d) => (!d.smd || d.layer === layer) && Math.hypot(d.at.x - p.x, d.at.y - p.y) <= Math.min(d.w, d.h) / 2);
  const viaAt = (p: Point, layer: LayerId) => vias.find((v) => v.layers.includes(layer) && Math.hypot(v.pos.x - p.x, v.pos.y - p.y) <= v.r);
  const near = (p: Point, layer: LayerId, r: number) => {
    if (!r) return byEnd.get(`${layer}|${key(p)}`) ?? [];
    return all.filter((w) => w.layer === layer && w.ends.some((e) => Math.hypot(e.x - p.x, e.y - p.y) <= r));
  };

  const out = new Set<ItemId>();
  const queue: Wire[] = [];
  const visit = (w: Wire) => {
    if (out.has(w.id)) return;
    out.add(w.id);
    queue.push(w);
  };
  const byId = new Map(all.map((w) => [w.id, w]));
  for (const id of start) {
    const w = byId.get(id);
    if (w) visit(w);
    const v = vias.find((x) => x.id === id);
    if (v && scope !== "chain") {
      out.add(v.id);
      v.layers.flatMap((l) => near(v.pos, l, v.r)).forEach(visit);
    }
  }
  while (queue.length) {
    const w = queue.shift()!;
    for (const p of w.ends) {
      if (padAt(p, w.layer)) continue;
      const via = viaAt(p, w.layer);
      if (via) {
        if (scope === "chain" || out.has(via.id)) continue;
        const others = via.layers.flatMap((l) => near(via.pos, l, via.r)).filter((o) => o.id !== w.id && !out.has(o.id));
        if (scope === "unroute") {
          const poured = pours.some((z) => z.net === via.net && via.layers.includes(z.layer) && inPolygon(via.pos, z.outline));
          if (others.length > 1 || poured) continue;
        }
        out.add(via.id);
        others.forEach(visit);
        continue;
      }
      const others = near(p, w.layer, 0).filter((o) => o.id !== w.id);
      if (scope !== "connection" && others.length !== 1) continue;
      others.forEach(visit);
    }
  }
  return out;
}

/** Tracks and vias routed from the selected footprints' pads, up to the first junction, pour or other pad. */
export function unrouteFootprints(items: Items, footprints: Footprints, ids: Iterable<ItemId>): ItemId[] {
  const start: ItemId[] = [];
  const all = wires(items);
  for (const id of ids) {
    const fp = items.get(id);
    if (fp?.kind !== "footprint") continue;
    for (const pad of worldPads(fp, id, footprints)) {
      if (!pad.net) continue;
      for (const w of all) {
        if (w.net !== pad.net || (pad.smd && w.layer !== pad.layer)) continue;
        if (w.ends.some((e) => Math.hypot(e.x - pad.at.x, e.y - pad.at.y) <= Math.max(pad.w, pad.h) / 2)) start.push(w.id);
      }
    }
  }
  return [...walkCopper(items, footprints, start, "unroute")];
}

// ---------- Nets panel ----------

export interface NetRow {
  name: string;
  cls: string;
  pads: number;
  vias: number;
  /** Routed copper length (tracks + arcs), nm. */
  length: number;
  /** Routed length per copper layer, nm. */
  byLayer: Map<LayerId, number>;
  unrouted: number;
}

const natural = new Intl.Collator(undefined, { numeric: true, sensitivity: "base" });

/** Per-net statistics computed from the board items and the live ratsnest. */
export function netRows(items: Items, airwires: readonly AirWire[]): { rows: NetRow[]; classes: string[] } {
  let classes: { name: string; nets: string[] }[] = [];
  for (const it of items.values()) if (it.kind === "board") classes = it.netClasses;
  const classOf = new Map<string, string>();
  for (const c of classes) for (const n of c.nets) classOf.set(n, c.name);
  const fallback = classes[0]?.name ?? "Default";
  const rows = new Map<string, NetRow>();
  const row = (name: string) => {
    let r = rows.get(name);
    if (!r) rows.set(name, (r = { name, cls: classOf.get(name) ?? fallback, pads: 0, vias: 0, length: 0, byLayer: new Map(), unrouted: 0 }));
    return r;
  };
  const routed = (net: string, layer: LayerId, len: number) => {
    const r = row(net);
    r.length += len;
    r.byLayer.set(layer, (r.byLayer.get(layer) ?? 0) + len);
  };
  for (const it of items.values()) {
    if (it.kind === "footprint") {
      for (const n of Object.values(it.padNets)) if (n) row(n).pads++;
    } else if (it.kind === "track" && it.net) routed(it.net, it.layer, Math.hypot(it.b.x - it.a.x, it.b.y - it.a.y));
    else if (it.kind === "arcTrack" && it.net) routed(it.net, it.layer, (Math.abs(it.sweep) * Math.PI * it.radius) / 180);
    else if (it.kind === "via" && it.net) row(it.net).vias++;
  }
  for (const w of airwires) if (w.net) row(w.net).unrouted++;
  return { rows: [...rows.values()].sort((a, b) => natural.compare(a.name, b.name)), classes: classes.map((c) => c.name) };
}
