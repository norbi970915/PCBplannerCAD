// Polygon editing on boards: outline handles (used by the select tool) and
// the boundary reshape tool — click the outline, add corners, click the
// outline again; the new path replaces that stretch of the boundary.

import type { Item, ItemId, Point } from "../../core/types";
import { editableRing } from "../polyedit";
import type { DrawLine, Rgba } from "../renderer";
import type { ToolEnv } from "./types";

export const HANDLE_PX = 7;
const HANDLE: Rgba = [0.3, 0.62, 1, 1];
const HANDLE_IN: Rgba = [1, 1, 1, 1];
/** Square vertex handles and small edge-midpoint handles. */
export function ringHandles(ring: Point[], px: number, midpoints = true): DrawLine[] {
  const out: DrawLine[] = [];
  const square = (c: Point, r: number, color: Rgba, fill: Rgba) => {
    // A zero-length line is a filled dot; the frame makes it a handle.
    out.push({ a: c, b: c, width: 2 * r, color: fill });
    const s = [
      { x: c.x - r, y: c.y - r },
      { x: c.x + r, y: c.y - r },
      { x: c.x + r, y: c.y + r },
      { x: c.x - r, y: c.y + r },
    ];
    s.forEach((p, i) => out.push({ a: p, b: s[(i + 1) % 4], width: 1.5 * px, color }));
  };
  ring.forEach((p, i) => {
    square(p, (HANDLE_PX / 2) * px, HANDLE, HANDLE_IN);
    if (midpoints) {
      const q = ring[(i + 1) % ring.length];
      square({ x: (p.x + q.x) / 2, y: (p.y + q.y) / 2 }, 2.5 * px, HANDLE, HANDLE);
    }
  });
  return out;
}

export function ringLines(ring: Point[], px: number, color: Rgba, closed = true, width = 1.5): DrawLine[] {
  const out: DrawLine[] = [];
  const n = closed ? ring.length : ring.length - 1;
  for (let i = 0; i < n; i++) out.push({ a: ring[i], b: ring[(i + 1) % ring.length], width: width * px, color });
  return out;
}

/** Writes the outline and dependent fills in one undo transaction. */
export function commitRing(env: ToolEnv, id: ItemId, item: Item, ring: Point[]) {
  const original = editableRing(item);
  return original ? env.exec({ op: "editPolygon", id, original, outlines: [ring] }) : Promise.resolve(false);
}
