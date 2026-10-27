// Dimension geometry (mirrors crates/core/src/pcb/dimension.rs): extension lines,
// the dimension line with arrows, and the value text.

import type { DimensionItem, Item, Point } from "../core/types";

type V = { x: number; y: number };

export function dimensionDirection(d: DimensionItem): V {
  if (!d.orthogonal) {
    const dx = d.b.x - d.a.x, dy = d.b.y - d.a.y, l = Math.hypot(dx, dy);
    if (l > 0) return { x: dx / l, y: dy / l };
  }
  const r = (d.angle * Math.PI) / 180;
  return { x: Math.cos(r), y: Math.sin(r) };
}

export function dimensionValue(d: DimensionItem): number {
  const u = dimensionDirection(d);
  return Math.abs((d.b.x - d.a.x) * u.x + (d.b.y - d.a.y) * u.y);
}

export function dimensionLabel(d: DimensionItem): string {
  const v = dimensionValue(d), p = Math.min(d.precision, 6);
  if (d.units === "mil") return `${(v / 25_400).toFixed(p)} mil`;
  if (d.units === "inch") return `${(v / 25_400_000).toFixed(p)} in`;
  return `${(v / 1e6).toFixed(p)} mm`;
}

/** Lines and text of a dimension, in board coordinates. */
export function dimensionPrims(d: DimensionItem): Item[] {
  const u = dimensionDirection(d), n = { x: -u.y, y: u.x };
  const along = (d.b.x - d.a.x) * u.x + (d.b.y - d.a.y) * u.y;
  const p1 = { x: d.a.x + n.x * d.offset, y: d.a.y + n.y * d.offset };
  const p2 = { x: p1.x + u.x * along, y: p1.y + u.y * along };
  const h = d.textHeight, side = d.offset >= 0 ? 1 : -1, over = h * 0.4;
  const pt = (p: V): Point => ({ x: Math.round(p.x), y: Math.round(p.y) });
  const line = (a: V, b: V): Item => ({ kind: "line", a: pt(a), b: pt(b), width: d.width, layer: d.layer });
  const out: Item[] = [];
  for (const [from, foot] of [[d.a, p1], [d.b, p2]] as const) {
    const dx = foot.x - from.x, dy = foot.y - from.y, l = Math.hypot(dx, dy);
    if (l > 0) out.push(line(from, { x: foot.x + (dx / l) * over, y: foot.y + (dy / l) * over }));
  }
  if (along !== 0) {
    out.push(line(p1, p2));
    const s = Math.sign(along), arrow = Math.min(h * 0.8, Math.abs(along) / 3);
    const c = Math.cos((25 * Math.PI) / 180), sn = Math.sin((25 * Math.PI) / 180);
    for (const [tip, dir] of [[p1, s], [p2, -s]] as const) {
      for (const k of [-1, 1]) {
        const dx = u.x * dir * c - u.y * dir * sn * k, dy = u.y * dir * c + u.x * dir * sn * k;
        out.push(line(tip, { x: tip.x + dx * arrow, y: tip.y + dy * arrow }));
      }
    }
  }
  let rot = (Math.atan2(u.y, u.x) * 180) / Math.PI;
  if (rot > 90 + 1e-6) rot -= 180;
  else if (rot <= -90 + 1e-6) rot += 180;
  out.push({
    kind: "text",
    pos: pt({ x: (p1.x + p2.x) / 2 + n.x * side * h * 0.8, y: (p1.y + p2.y) / 2 + n.y * side * h * 0.8 }),
    text: dimensionLabel(d),
    height: h,
    rotation: Math.abs(rot) < 1e-9 ? 0 : rot,
    hAlign: "center",
    vAlign: "middle",
    layer: d.layer,
    mirrored: false,
  });
  return out;
}

/**
 * The dimension for measured points `a`, `b` and a third point the dimension line passes through.
 * `style`: aligned with a→b, or measuring only X / only Y.
 */
export function dimensionThrough(a: Point, b: Point, through: Point, style: "aligned" | "horizontal" | "vertical", base: Omit<DimensionItem, "a" | "b" | "offset" | "orthogonal" | "angle">): DimensionItem {
  const orthogonal = style !== "aligned";
  const angle = style === "vertical" ? 90 : 0;
  const d: DimensionItem = { ...base, a, b, offset: 0, orthogonal, angle };
  const u = dimensionDirection(d);
  d.offset = Math.round((through.x - a.x) * -u.y + (through.y - a.y) * u.x);
  return d;
}
