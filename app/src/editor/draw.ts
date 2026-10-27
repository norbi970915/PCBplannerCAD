// Converts items into colorless primitives (stroked segments and filled
// triangles) tagged with a *role*; the theme maps roles to colors. The same
// primitives drive rendering, hit testing and bounds.

import { dimensionPrims } from "./dimension";
import type {
  CachedPart,
  Footprint,
  DocKind,
  Field,
  HAlign,
  Item,
  ItemOf,
  LibSymbol,
  PinItem,
  Point,
  PortShape,
  VAlign,
} from "../core/types";
import { NM_PER_MM } from "../core/units";
import { layerVisible, type ViewSettings } from "../core/view";
import { textSegments, textStroke, textWidth } from "./font";
import {
  arcPoints,
  circlePoints,
  polar,
  rectPoints,
  rotatePoint,
  roundRectPoints,
  triangulate,
  triangulateWithHoles,
} from "./shapes";

export type Role = string;

export interface Seg {
  a: Point;
  b: Point;
  w: number;
  role: Role;
}

export interface Fill {
  tris: number[];
  role: Role;
}

export interface Prims {
  segs: Seg[];
  fills: Fill[];
}

export interface DrawContext {
  kind: DocKind;
  cache: Record<string, CachedPart>;
  /** Symbol editor: symbol being edited (pin display flags, field templates). */
  symbol?: LibSymbol | null;
  /** PCB: footprint geometry for placed footprints. */
  footprints?: Record<string, Footprint>;
  /** PCB: false draws pours as outlines only. */
  pourFills?: boolean;
  view?: ViewSettings;
}

const mm = (v: number) => v * NM_PER_MM;

export const SCH = {
  line: mm(0.15),
  wire: mm(0.22),
  bus: mm(0.7),
  junction: mm(0.5),
  pin: mm(0.15),
  pinName: mm(1.27),
  pinNumber: mm(1.0),
  text: mm(1.27),
  noConnect: mm(0.65),
  bubble: mm(0.6),
};

/** Upright text direction for an arbitrary angle (0 or 90). */
const uprightAngle = (deg: number) => {
  const d = ((deg % 180) + 180) % 180;
  return d > 45 && d <= 135 ? 90 : 0;
};

const norm = (a: number) => ((a % 360) + 360) % 360;

class Out implements Prims {
  segs: Seg[] = [];
  fills: Fill[] = [];

  seg(a: Point, b: Point, w: number, role: Role) {
    this.segs.push({ a, b, w, role });
  }

  path(pts: Point[], w: number, role: Role, closed = false) {
    for (let i = 1; i < pts.length; i++) this.seg(pts[i - 1], pts[i], w, role);
    if (closed && pts.length > 2) this.seg(pts[pts.length - 1], pts[0], w, role);
  }

  fill(pts: Point[], role: Role) {
    const tris = triangulate(pts);
    if (tris.length) this.fills.push({ tris, role });
  }

  text(text: string, pos: Point, height: number, rotation: number, h: HAlign, v: VAlign, role: Role, mirrored = false) {
    if (!text) return;
    const w = textStroke(height);
    for (const [a, b] of textSegments(text, pos, { height, rotation, hAlign: h, vAlign: v, mirrored })) this.seg(a, b, w, role);
  }
}

// ---------- placement (symbol local → world) ----------

export interface Placement {
  pos: Point;
  rotation: number;
  mirror: boolean;
}

export function place(p: Point, pl: Placement): Point {
  const m = pl.mirror ? { x: -p.x, y: p.y } : p;
  const r = rotatePoint(m, { x: 0, y: 0 }, pl.rotation);
  return { x: r.x + pl.pos.x, y: r.y + pl.pos.y };
}

export function placeAngle(a: number, pl: Placement): number {
  return norm((pl.mirror ? 180 - a : a) + pl.rotation);
}

/** A symbol-local item moved into world coordinates. */
export function placeItem(item: Item, pl: Placement): Item {
  const p = (q: Point) => place(q, pl);
  switch (item.kind) {
    case "line":
      return { ...item, a: p(item.a), b: p(item.b) };
    case "arc": {
      const start = pl.mirror ? 180 - item.startAngle - item.sweep : item.startAngle;
      return { ...item, center: p(item.center), startAngle: norm(start + pl.rotation) };
    }
    case "circle":
      return { ...item, center: p(item.center) };
    case "rect": {
      const corners = [item.a, { x: item.b.x, y: item.a.y }, item.b, { x: item.a.x, y: item.b.y }].map(p);
      if (pl.rotation % 90 === 0) return { ...item, a: p(item.a), b: p(item.b) };
      return { kind: "polyline", points: corners, closed: true, width: item.width, layer: item.layer, fill: item.fill };
    }
    case "polyline":
      return { ...item, points: item.points.map(p) };
    case "text":
      return { ...item, pos: p(item.pos), rotation: placeAngle(item.rotation, pl) };
    case "pin":
      return { ...item, pos: p(item.pos), rotation: placeAngle(item.rotation, pl) };
    case "pad":
      return { ...item, pos: p(item.pos), rotation: placeAngle(item.rotation, pl) };
    default:
      return item;
  }
}

// ---------- shared graphics ----------

function strokeWidth(w: number, kind: DocKind) {
  if (w > 0) return w;
  return kind === "schematic" || kind === "symbol" ? SCH.line : mm(0.1);
}

function drawGraphic(out: Out, item: Item, ctx: DrawContext, role: Role, fillRole: Role) {
  switch (item.kind) {
    case "line":
      out.seg(item.a, item.b, strokeWidth(item.width, ctx.kind), role);
      break;
    case "arc":
      out.path(arcPoints(item.center, item.radius, item.startAngle, item.sweep), strokeWidth(item.width, ctx.kind), role);
      break;
    case "circle": {
      const pts = circlePoints(item.center, item.radius);
      if (item.fill !== "none") out.fill(pts, item.fill === "background" ? fillRole : role);
      if (item.width > 0 || item.fill === "none" || ctx.kind === "symbol" || ctx.kind === "schematic") {
        out.path(pts, strokeWidth(item.width, ctx.kind), role, true);
      }
      break;
    }
    case "rect": {
      const pts = [item.a, { x: item.b.x, y: item.a.y }, item.b, { x: item.a.x, y: item.b.y }];
      if (item.fill !== "none") out.fill(pts, item.fill === "background" ? fillRole : role);
      if (item.width > 0 || item.fill === "none" || ctx.kind === "symbol" || ctx.kind === "schematic") {
        out.path(pts, strokeWidth(item.width, ctx.kind), role, true);
      }
      break;
    }
    case "polyline":
      if (item.fill !== "none" && item.points.length > 2) out.fill(item.points, item.fill === "background" ? fillRole : role);
      if (item.width > 0 || item.fill === "none" || ctx.kind === "symbol" || ctx.kind === "schematic") {
        out.path(item.points, strokeWidth(item.width, ctx.kind), role, item.closed);
      }
      break;
    case "text":
      out.text(item.text, item.pos, item.height, item.rotation, item.hAlign, item.vAlign, role, item.mirrored);
      break;
  }
}

// ---------- pins ----------

function drawPin(out: Out, pin: PinItem, sym: LibSymbol | null | undefined, showHidden: boolean) {
  if (pin.hidden && !showHidden) return;
  const role = pin.hidden ? "pinHidden" : "pin";
  const dir = pin.rotation;
  const end = rotatePoint({ x: pin.pos.x + pin.length, y: pin.pos.y }, pin.pos, dir);
  const unit = { x: (end.x - pin.pos.x) / (pin.length || 1), y: (end.y - pin.pos.y) / (pin.length || 1) };
  const inverted = pin.shape === "inverted" || pin.shape === "invertedClock";
  const clock = pin.shape === "clock" || pin.shape === "invertedClock";
  const lineEnd = inverted ? { x: end.x - unit.x * SCH.bubble * 2, y: end.y - unit.y * SCH.bubble * 2 } : end;
  if (pin.length > 0) out.seg(pin.pos, lineEnd, SCH.pin, role);
  if (inverted) {
    const c = { x: end.x - unit.x * SCH.bubble, y: end.y - unit.y * SCH.bubble };
    out.path(circlePoints(c, SCH.bubble, 20), SCH.pin, role, true);
  }
  if (clock) {
    const perp = { x: -unit.y, y: unit.x };
    const s = mm(0.6);
    out.path(
      [
        { x: end.x + perp.x * s, y: end.y + perp.y * s },
        { x: end.x + unit.x * s * 1.3, y: end.y + unit.y * s * 1.3 },
        { x: end.x - perp.x * s, y: end.y - perp.y * s },
      ],
      SCH.pin,
      role,
    );
  }

  const showNumbers = sym?.showPinNumbers ?? true;
  const showNames = sym?.showPinNames ?? true;
  const nameOffset = sym?.pinNameOffset ?? mm(0.508);
  const textRot = uprightAngle(dir);
  const mid = { x: (pin.pos.x + end.x) / 2, y: (pin.pos.y + end.y) / 2 };
  // "Above" the pin line in reading orientation (left of a vertical pin).
  const above = textRot === 0 ? { x: 0, y: 1 } : { x: -1, y: 0 };
  const gap = mm(0.3);
  const name = pin.name === "~" ? "" : pin.name;

  if (showNumbers && pin.number) {
    const p = { x: mid.x + above.x * gap, y: mid.y + above.y * gap };
    out.text(pin.number, p, SCH.pinNumber, textRot, "center", "bottom", "pinNum");
  }
  if (showNames && name) {
    if (nameOffset > 0) {
      const p = { x: end.x + unit.x * nameOffset, y: end.y + unit.y * nameOffset };
      // Text flows from the body end into the body.
      const forward = textRot === 0 ? unit.x >= 0 : unit.y >= 0;
      out.text(name, p, SCH.pinName, textRot, forward ? "left" : "right", "middle", "pinName");
    } else {
      const p = { x: mid.x - above.x * gap, y: mid.y - above.y * gap };
      out.text(name, p, SCH.pinName, textRot, "center", "top", "pinName");
    }
  }
}

// ---------- schematic objects ----------

const unitLetter = (u: number) => (u >= 1 && u <= 26 ? String.fromCharCode(64 + u) : `.${u}`);

export function fieldText(part: ItemOf<"part">, f: Field, cached?: CachedPart): string {
  if (f.name === "Designator" && cached && cached.symbol.units.length > 1) return `${f.value}${unitLetter(part.unit)}`;
  return f.value;
}

function drawPart(out: Out, part: ItemOf<"part">, ctx: DrawContext) {
  const cached = ctx.cache[part.libRef];
  const pl: Placement = { pos: part.pos, rotation: part.rotation, mirror: part.mirror };
  if (!cached) {
    // Missing library data: draw a placeholder box so the part stays selectable.
    const s = mm(2.54);
    const pts = [
      { x: -s, y: -s },
      { x: s, y: -s },
      { x: s, y: s },
      { x: -s, y: s },
    ].map((p) => place(p, pl));
    out.path(pts, SCH.line, "error", true);
    out.text("?", part.pos, SCH.text, 0, "center", "middle", "error");
  } else {
    const unit = cached.symbol.units[part.unit - 1] ?? cached.symbol.units[0];
    for (const it of unit?.items ?? []) {
      const w = placeItem(it, pl);
      if (w.kind === "pin") drawPin(out, w, cached.symbol, false);
      else drawGraphic(out, w, ctx, w.kind === "text" ? "bodyText" : "body", "bodyFill");
    }
  }
  for (const f of part.fields) {
    if (!f.visible || !f.value) continue;
    const role = f.name === "Designator" ? "designator" : f.name === "Value" ? "value" : "field";
    out.text(fieldText(part, f, cached), { x: part.pos.x + f.offset.x, y: part.pos.y + f.offset.y }, f.height, f.rotation, f.hAlign, f.vAlign, role);
  }
}

/** Polygon of a global/hierarchical label body in its local frame (x = text direction). */
function labelOutline(shape: PortShape, len: number, h: number, hier: boolean): Point[] {
  const t = h / 2;
  if (hier) {
    // Small arrow/box at the connection point; text follows after it.
    const s = h;
    switch (shape) {
      case "input":
        return [{ x: 0, y: 0 }, { x: s / 2, y: t }, { x: s, y: t }, { x: s, y: -t }, { x: s / 2, y: -t }];
      case "output":
        return [{ x: 0, y: t }, { x: s / 2, y: t }, { x: s, y: 0 }, { x: s / 2, y: -t }, { x: 0, y: -t }];
      case "bidirectional":
      case "triState":
        return [{ x: 0, y: 0 }, { x: s / 2, y: t }, { x: s, y: 0 }, { x: s / 2, y: -t }];
      default:
        return [{ x: 0, y: t }, { x: s, y: t }, { x: s, y: -t }, { x: 0, y: -t }];
    }
  }
  switch (shape) {
    case "input":
      return [{ x: 0, y: 0 }, { x: t, y: t }, { x: len, y: t }, { x: len, y: -t }, { x: t, y: -t }];
    case "output":
      return [{ x: 0, y: t }, { x: len - t, y: t }, { x: len, y: 0 }, { x: len - t, y: -t }, { x: 0, y: -t }];
    case "bidirectional":
    case "triState":
      return [{ x: 0, y: 0 }, { x: t, y: t }, { x: len - t, y: t }, { x: len, y: 0 }, { x: len - t, y: -t }, { x: t, y: -t }];
    default:
      return [{ x: 0, y: t }, { x: len, y: t }, { x: len, y: -t }, { x: 0, y: -t }];
  }
}

function drawLabel(out: Out, l: ItemOf<"netLabel">) {
  const h = SCH.text;
  const dir = norm(l.rotation);
  const horizontal = uprightAngle(dir) === 0;
  const forward = horizontal ? dir < 90 || dir > 270 : dir < 180;
  if (l.scope === "local") {
    const gap = mm(0.25);
    const p = horizontal ? { x: l.pos.x, y: l.pos.y + gap } : { x: l.pos.x - gap, y: l.pos.y };
    out.text(l.text, p, h, horizontal ? 0 : 90, forward ? "left" : "right", "bottom", "label");
    return;
  }
  const hier = l.scope === "hierarchical";
  const role = hier ? "hierLabel" : "globalLabel";
  const textLen = textWidth(l.text, h);
  const len = textLen + h * 1.6;
  const local = labelOutline(l.shape, len, h * 1.8, hier);
  const tf = (p: Point) => rotatePoint({ x: l.pos.x + p.x, y: l.pos.y + p.y }, l.pos, dir);
  out.path(local.map(tf), SCH.line, role, true);
  const along = hier ? h * 1.8 + mm(0.4) : len / 2;
  const anchor = tf({ x: along, y: 0 });
  const align: HAlign = hier ? (forward ? "left" : "right") : "center";
  out.text(l.text, anchor, h, horizontal ? 0 : 90, align, "middle", role);
}

function drawPower(out: Out, p: ItemOf<"powerPort">) {
  const dir = norm(p.rotation);
  const tf = (x: number, y: number) => rotatePoint({ x: p.pos.x + x, y: p.pos.y + y }, p.pos, dir);
  const w = SCH.line;
  const r = "power";
  const stem = mm(1.27);
  let textAt = mm(1.9);
  switch (p.style) {
    case "ground":
      out.seg(tf(0, 0), tf(stem, 0), w, r);
      out.seg(tf(stem, -mm(1.27)), tf(stem, mm(1.27)), w, r);
      out.seg(tf(stem + mm(0.5), -mm(0.8)), tf(stem + mm(0.5), mm(0.8)), w, r);
      out.seg(tf(stem + mm(1.0), -mm(0.3)), tf(stem + mm(1.0), mm(0.3)), w, r);
      textAt = stem + mm(1.6);
      break;
    case "signalGround":
      out.seg(tf(0, 0), tf(stem, 0), w, r);
      out.path([tf(stem, -mm(1.0)), tf(stem, mm(1.0)), tf(stem + mm(1.2), 0)], w, r, true);
      textAt = stem + mm(1.8);
      break;
    case "earth":
      out.seg(tf(0, 0), tf(stem, 0), w, r);
      out.seg(tf(stem, -mm(1.27)), tf(stem, mm(1.27)), w, r);
      for (const y of [-1.0, -0.2, 0.6]) out.seg(tf(stem, mm(y)), tf(stem + mm(0.7), mm(y - 0.5)), w, r);
      textAt = stem + mm(1.4);
      break;
    case "arrow":
      out.seg(tf(0, 0), tf(stem, 0), w, r);
      out.fill([tf(stem, -mm(0.6)), tf(stem, mm(0.6)), tf(stem + mm(1.0), 0)], r);
      textAt = stem + mm(1.5);
      break;
    case "circle":
      out.seg(tf(0, 0), tf(stem, 0), w, r);
      out.path(circlePoints(tf(stem + mm(0.5), 0), mm(0.5), 20), w, r, true);
      textAt = stem + mm(1.5);
      break;
    case "wave": {
      out.seg(tf(0, 0), tf(stem, 0), w, r);
      const pts: Point[] = [];
      for (let i = 0; i <= 16; i++) {
        const y = -mm(1.0) + (mm(2.0) * i) / 16;
        pts.push(tf(stem + mm(0.35) * Math.sin((i / 16) * Math.PI * 2), y));
      }
      out.path(pts, w, r);
      textAt = stem + mm(0.9);
      break;
    }
    default:
      out.seg(tf(0, 0), tf(stem, 0), w, r);
      out.seg(tf(stem, -mm(1.0)), tf(stem, mm(1.0)), w, r);
      textAt = stem + mm(0.5);
  }
  const anchor = tf(textAt, 0);
  const d = dir;
  let h: HAlign = "center";
  let v: VAlign = "bottom";
  if (d > 45 && d <= 135) v = "bottom";
  else if (d > 225 && d <= 315) v = "top";
  else if (d > 135 && d <= 225) [h, v] = ["right", "middle"];
  else [h, v] = ["left", "middle"];
  out.text(p.net, anchor, SCH.text, 0, h, v, r);
}

function drawSheet(out: Out, s: ItemOf<"sheet">) {
  const tl = s.pos;
  const pts = [tl, { x: tl.x + s.size.x, y: tl.y }, { x: tl.x + s.size.x, y: tl.y - s.size.y }, { x: tl.x, y: tl.y - s.size.y }];
  out.fill(pts, "sheetFill");
  out.path(pts, mm(0.25), "sheet", true);
  out.text(s.name, { x: tl.x, y: tl.y + mm(0.6) }, SCH.text, 0, "left", "bottom", "sheetName");
  out.text(`File: ${s.file}`, { x: tl.x, y: tl.y - s.size.y - mm(0.6) }, mm(1.0), 0, "left", "top", "sheetFile");
  for (const e of s.entries) {
    const p = sheetEntryPoint(s, e);
    const inward = e.side === "left" ? 0 : e.side === "right" ? 180 : e.side === "top" ? 270 : 90;
    const h = mm(1.0);
    const local = labelOutline(e.shape, h * 1.6, h * 1.4, true);
    out.path(local.map((q) => rotatePoint({ x: p.x + q.x, y: p.y + q.y }, p, inward)), SCH.line, "sheetEntry", true);
    const tp = polar(p, h * 1.6 + mm(0.4), inward);
    const rot = uprightAngle(inward);
    const forward = inward === 0 || inward === 90;
    out.text(e.name, tp, SCH.text, rot, forward ? "left" : "right", "middle", "sheetEntry");
  }
}

export function sheetEntryPoint(s: ItemOf<"sheet">, e: ItemOf<"sheet">["entries"][number]): Point {
  switch (e.side) {
    case "left":
      return { x: s.pos.x, y: s.pos.y - e.offset };
    case "right":
      return { x: s.pos.x + s.size.x, y: s.pos.y - e.offset };
    case "top":
      return { x: s.pos.x + e.offset, y: s.pos.y };
    default:
      return { x: s.pos.x + e.offset, y: s.pos.y - s.size.y };
  }
}

// ---------- pads ----------

function drawPad(out: Out, p: ItemOf<"pad">, view?: ViewSettings, netName = "") {
  if (view?.pads === false || !layerVisible(view, p.layer)) return;
  const role = `L${p.layer}`;
  const c = p.pos;
  switch (p.shape) {
    case "circle":
      out.fill(circlePoints(c, p.width / 2, 32), role);
      break;
    case "oval": {
      const long = Math.max(p.width, p.height);
      const short = Math.min(p.width, p.height);
      const along = (p.width >= p.height ? 0 : 90) + p.rotation;
      const half = (long - short) / 2;
      const a = polar(c, half, along + 180);
      const b = polar(c, half, along);
      out.seg(a, b, short, role);
      break;
    }
    case "roundRect":
      out.fill(roundRectPoints(c, p.width, p.height, p.rotation, Math.min(p.width, p.height) * p.cornerRatio), role);
      break;
    default:
      out.fill(rectPoints(c, p.width, p.height, p.rotation), role);
  }
  if (p.drill) {
    const { width, height } = p.drill;
    if (width === height) out.fill(circlePoints(c, width / 2, 24), "hole");
    else {
      const along = (width >= height ? 0 : 90) + p.rotation;
      const half = (Math.max(width, height) - Math.min(width, height)) / 2;
      out.seg(polar(c, half, along + 180), polar(c, half, along), Math.min(width, height), "hole");
    }
  }
  const showNumber = !!p.number && view?.padNumbers !== false;
  const showNet = !!netName && view?.padNetNames === true;
  if (showNumber || showNet) {
    // Fit the (horizontal) number inside the pad's horizontal/vertical extent.
    const r = (p.rotation * Math.PI) / 180;
    const w = p.width;
    const ht = p.shape === "circle" ? p.width : p.height;
    const across = Math.abs(w * Math.cos(r)) + Math.abs(ht * Math.sin(r));
    const tall = Math.abs(w * Math.sin(r)) + Math.abs(ht * Math.cos(r));
    const h = Math.min(mm(1.2), tall * 0.55, (across * 0.8) / Math.max(1, p.number.length * 0.85));
    if (showNumber && h > 0) out.text(p.number, showNet ? { x: c.x, y: c.y + tall * 0.2 } : c, showNet ? h * 0.65 : h, 0, "center", "middle", "padNum");
    if (showNet) {
      const nh = Math.min(mm(0.8), tall * (showNumber ? 0.3 : 0.5), across * 0.8 / Math.max(1, netName.length * 0.85));
      out.text(netName, showNumber ? { x: c.x, y: c.y - tall * 0.2 } : c, nh, 0, "center", "middle", "padNum");
    }
  }
}

// ---------- entry point ----------

export function drawItem(item: Item, ctx: DrawContext): Prims {
  const out = new Out();
  const sch = ctx.kind === "schematic" || ctx.kind === "symbol";
  if (!sch) {
    if ("layer" in item && !layerVisible(ctx.view, item.layer)) return out;
    if ((item.kind === "track" || item.kind === "arcTrack") && ctx.view?.tracks === false) return out;
    if (item.kind === "via" && (ctx.view?.vias === false || !layerVisible(ctx.view, 15))) return out;
    if (item.kind === "text" && ctx.view?.text === false) return out;
    if (["line", "arc", "circle", "rect", "polyline", "dimension"].includes(item.kind) && ctx.view?.graphics === false) return out;
    if (item.kind === "zone" && item.keepout && !layerVisible(ctx.view, 5)) return out;
  }
  switch (item.kind) {
    case "line":
    case "arc":
    case "circle":
    case "rect":
    case "polyline":
    case "text":
      if (ctx.kind === "symbol") drawGraphic(out, item, ctx, item.kind === "text" ? "bodyText" : "body", "bodyFill");
      else if (sch) drawGraphic(out, item, ctx, item.kind === "text" ? "text" : "graphic", "graphicFill");
      else drawGraphic(out, item, ctx, `L${item.layer}`, `L${item.layer}`);
      break;
    case "pin":
      drawPin(out, item, ctx.symbol, true);
      break;
    case "pad":
      drawPad(out, item, ctx.view);
      break;
    case "wire":
      out.seg(item.a, item.b, SCH.wire, "wire");
      break;
    case "bus":
      out.seg(item.a, item.b, SCH.bus, "bus");
      break;
    case "busEntry":
      out.seg(item.pos, { x: item.pos.x + item.size.x, y: item.pos.y + item.size.y }, SCH.wire, "wire");
      break;
    case "junction":
      out.fill(circlePoints(item.pos, SCH.junction, 20), "wire");
      break;
    case "noConnect": {
      const s = SCH.noConnect;
      out.seg({ x: item.pos.x - s, y: item.pos.y - s }, { x: item.pos.x + s, y: item.pos.y + s }, SCH.line, "noConnect");
      out.seg({ x: item.pos.x - s, y: item.pos.y + s }, { x: item.pos.x + s, y: item.pos.y - s }, SCH.line, "noConnect");
      break;
    }
    case "netLabel":
      drawLabel(out, item);
      break;
    case "powerPort":
      drawPower(out, item);
      break;
    case "part":
      drawPart(out, item, ctx);
      break;
    case "sheet":
      drawSheet(out, item);
      break;
    case "footprint":
      drawFootprint(out, item, ctx);
      break;
    case "track":
      out.seg(item.a, item.b, item.width, `L${item.layer}`);
      break;
    case "arcTrack":
      out.path(arcPoints(item.center, item.radius, item.startAngle, item.sweep), item.width, `L${item.layer}`);
      break;
    case "via":
      out.fill(circlePoints(item.pos, item.diameter / 2, 24), "via");
      out.fill(circlePoints(item.pos, item.drill / 2, 16), "hole");
      break;
    case "zone":
      drawZone(out, item, ctx.pourFills !== false);
      break;
    case "dimension":
      for (const p of dimensionPrims(item)) drawGraphic(out, p, ctx, `L${item.layer}`, `L${item.layer}`);
      break;
    case "board":
      break;
  }
  return out;
}

// ---------- board ----------

const FLIP: Record<number, number> = { 0: 1, 1: 0, 2: 3, 3: 2, 6: 7, 7: 6, 8: 9, 9: 8, 10: 11, 11: 10, 12: 13, 13: 12 };
const flipLayer = (l: number) => FLIP[l] ?? l;

function withLayer(item: Item, f: (l: number) => number): Item {
  return "layer" in item && typeof item.layer === "number" ? ({ ...item, layer: f(item.layer) } as Item) : item;
}

/** A footprint placed on a board: local items → world, bottom side mirrored with layers flipped. */
function drawFootprint(out: Out, fp: ItemOf<"footprint">, ctx: DrawContext) {
  const geom = ctx.footprints?.[fp.footprint];
  const pl: Placement = { pos: fp.pos, rotation: fp.rotation, mirror: fp.side === "bottom" };
  if (!geom) {
    const s = mm(1.5);
    out.path([{ x: -s, y: -s }, { x: s, y: -s }, { x: s, y: s }, { x: -s, y: s }].map((p) => place(p, pl)), mm(0.1), "error", true);
    if (ctx.view?.references !== false) out.text(fp.designator, fp.pos, mm(1), 0, "center", "middle", "error");
    return;
  }
  for (const it of geom.items) {
    let w = placeItem(it, pl);
    if (pl.mirror) w = withLayer(w, flipLayer);
    if ("layer" in w && !layerVisible(ctx.view, w.layer)) continue;
    if (w.kind === "pad") drawPad(out, w, ctx.view, fp.padNets[w.number] ?? "");
    else if (w.kind === "text") {
      const ref = w.text === ".Designator" || w.text === fp.designator;
      const value = w.text === ".Value" || w.text === fp.value;
      if (ref ? ctx.view?.references === false : value ? ctx.view?.values === false : ctx.view?.text === false) continue;
      const text = w.text === ".Designator" ? fp.designator : w.text === ".Value" ? fp.value : w.text;
      out.text(text, w.pos, w.height, w.rotation, w.hAlign, w.vAlign, `L${w.layer}`, pl.mirror !== w.mirrored);
    } else if (ctx.view?.graphics !== false) drawGraphic(out, w, ctx, `L${(w as { layer: number }).layer}`, `L${(w as { layer: number }).layer}`);
  }
}

/** Pour fills are large and immutable; a repour replaces the polygons, so earcut runs once per fill. */
const fillTriangles = new WeakMap<object, number[]>();

function drawZone(out: Out, z: ItemOf<"zone">, fills: boolean) {
  // Outlines are construction lines, not copper: 1 px at any zoom.
  const outline = 0;
  if (z.keepout) {
    out.path(z.outline, outline, "keepout", true);
    const xs = z.outline.map((p) => p.x);
    const ys = z.outline.map((p) => p.y);
    const minX = Math.min(...xs), maxX = Math.max(...xs), minY = Math.min(...ys), maxY = Math.max(...ys);
    const step = Math.max(mm(1), (maxX - minX + maxY - minY) / 40);
    for (let d = minX - (maxY - minY); d < maxX; d += step) {
      const a = { x: d, y: minY };
      const b = { x: d + (maxY - minY), y: maxY };
      if (b.x > minX && a.x < maxX) out.seg(a, b, 0, "keepout");
    }
    return;
  }
  // Teardrops are part of the track joint: always shown, no construction outline.
  for (const poly of fills || z.teardrop ? z.fill : []) {
    let tris = fillTriangles.get(poly);
    if (!tris) fillTriangles.set(poly, (tris = triangulateWithHoles(poly.outer, poly.holes)));
    if (tris.length) out.fills.push({ tris, role: `L${z.layer}` });
  }
  if (!z.teardrop) out.path(z.outline, outline, `Z${z.layer}`, true);
}

/** Board snap targets: connection points plus corners and line ends. */
export function snapPoints(item: Item, ctx: DrawContext): Point[] {
  switch (item.kind) {
    case "zone":
      return item.outline;
    case "polyline":
      return item.points;
    case "line":
    case "dimension":
      return [item.a, item.b];
    case "arcTrack":
    case "arc": {
      const pts = arcPoints(item.center, item.radius, item.startAngle, item.sweep);
      return [pts[0], pts[pts.length - 1], item.center];
    }
    case "circle":
      return [item.center];
    case "rect":
      return [item.a, item.b, { x: item.a.x, y: item.b.y }, { x: item.b.x, y: item.a.y }];
    default:
      return connectionPoints(item, ctx);
  }
}

/** Connection points of an item (for snapping and wire auto-termination). */
export function connectionPoints(item: Item, ctx: DrawContext): Point[] {
  switch (item.kind) {
    case "wire":
    case "bus":
      return [item.a, item.b];
    case "busEntry":
      return [item.pos, { x: item.pos.x + item.size.x, y: item.pos.y + item.size.y }];
    case "junction":
    case "noConnect":
    case "netLabel":
    case "powerPort":
      return [item.pos];
    case "pin":
      return [item.pos];
    case "sheet":
      return item.entries.map((e) => sheetEntryPoint(item, e));
    case "track":
      return [item.a, item.b];
    case "via":
    case "pad":
      return [item.pos];
    case "footprint": {
      const geom = ctx.footprints?.[item.footprint];
      if (!geom) return [];
      const pl = { pos: item.pos, rotation: item.rotation, mirror: item.side === "bottom" };
      return geom.items.filter((i) => i.kind === "pad" && ctx.view?.pads !== false && layerVisible(ctx.view, pl.mirror ? flipLayer(i.layer) : i.layer)).map((p) => place((p as ItemOf<"pad">).pos, pl));
    }
    case "part": {
      const cached = ctx.cache[item.libRef];
      const unit = cached?.symbol.units[item.unit - 1];
      if (!unit) return [];
      const pl = { pos: item.pos, rotation: item.rotation, mirror: item.mirror };
      return unit.items.filter((i): i is PinItem => i.kind === "pin").map((pin) => place(pin.pos, pl));
    }
    default:
      return [];
  }
}
