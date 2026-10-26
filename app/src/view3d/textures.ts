// Paints a board face (copper under mask, exposed pads, silkscreen) into a
// canvas used as a texture on the 3D laminate.

import type { DocState } from "../core/store";
import type { BoardItem } from "../core/types";
import { drawItem, type DrawContext, type Prims } from "../editor/draw";

export interface FaceTexture {
  canvas: HTMLCanvasElement;
  /** World rectangle (nm) the canvas covers. */
  minX: number;
  minY: number;
  maxX: number;
  maxY: number;
}

const COPPER_EXPOSED = "#d4a94a";
const SILK_DEFAULT = "#f2f2f2";
const MAX_PX = 4096;

function shade(hex: string, k: number): string {
  const n = parseInt(hex.replace("#", "").padEnd(6, "0").slice(0, 6), 16);
  const ch = (s: number) => Math.max(0, Math.min(255, Math.round(((n >> s) & 255) * k)));
  return `rgb(${ch(16)},${ch(8)},${ch(0)})`;
}

/** Draws both faces; `bounds` in nm is the board outline's bounding box. */
export function paintFaces(doc: DocState, bounds: { minX: number; minY: number; maxX: number; maxY: number }): { top: FaceTexture; bottom: FaceTexture } {
  const settings = [...doc.items.values()].find((i): i is BoardItem => i.kind === "board");
  const mask = settings?.maskColor ?? "#1f5f2a";
  const silk = settings?.silkColor ?? SILK_DEFAULT;
  const ctx: DrawContext = { kind: "pcb", cache: doc.cache, footprints: doc.footprints };

  // Bucket primitives per role and whether they're exposed copper (pads).
  const prims: { prims: Prims; exposed: boolean }[] = [];
  for (const item of doc.items.values()) {
    if (item.kind === "board") continue;
    prims.push({ prims: drawItem(item, ctx), exposed: item.kind === "footprint" });
  }

  const w = bounds.maxX - bounds.minX;
  const h = bounds.maxY - bounds.minY;
  // ~20 px/mm, capped for GPU limits.
  const scale = Math.min(20 / 1e6, MAX_PX / Math.max(w, h));
  const pw = Math.max(1, Math.ceil(w * scale));
  const ph = Math.max(1, Math.ceil(h * scale));

  const face = (side: "top" | "bottom"): FaceTexture => {
    const canvas = document.createElement("canvas");
    canvas.width = pw;
    canvas.height = ph;
    const g = canvas.getContext("2d")!;
    // Y up in the design; canvas Y down. Bottom face is seen mirrored, but the
    // texture is mapped in board coordinates so no flip is needed here.
    g.setTransform(scale, 0, 0, -scale, -bounds.minX * scale, bounds.maxY * scale);
    g.lineCap = "round";
    g.lineJoin = "round";
    g.fillStyle = mask;
    g.fillRect(bounds.minX, bounds.minY, w, h);

    const copper = side === "top" ? ["L0", "L15"] : ["L1", "L15"];
    const silkRole = side === "top" ? "L2" : "L3";
    const paint = (p: Prims, roles: string[], color: string) => {
      g.fillStyle = color;
      g.strokeStyle = color;
      for (const f of p.fills) {
        if (!roles.includes(f.role)) continue;
        g.beginPath();
        for (let i = 0; i + 5 < f.tris.length; i += 6) {
          g.moveTo(f.tris[i], f.tris[i + 1]);
          g.lineTo(f.tris[i + 2], f.tris[i + 3]);
          g.lineTo(f.tris[i + 4], f.tris[i + 5]);
          g.closePath();
        }
        g.fill();
        // Hairline stroke hides triangle seams.
        g.lineWidth = 1 / scale;
        g.stroke();
      }
      for (const s of p.segs) {
        if (!roles.includes(s.role)) continue;
        g.lineWidth = Math.max(s.w, 1 / scale);
        g.beginPath();
        g.moveTo(s.a.x, s.a.y);
        g.lineTo(s.b.x, s.b.y);
        g.stroke();
      }
    };
    const underMask = shade(mask, 1.45);
    for (const { prims: p, exposed } of prims) if (!exposed) paint(p, copper, underMask);
    for (const { prims: p, exposed } of prims) if (!exposed) paint(p, ["via"], underMask);
    for (const { prims: p, exposed } of prims) if (exposed) paint(p, copper, COPPER_EXPOSED);
    for (const { prims: p } of prims) paint(p, [silkRole], silk);
    for (const { prims: p } of prims) paint(p, ["hole"], "#141414");
    return { canvas, ...bounds };
  };
  return { top: face("top"), bottom: face("bottom") };
}
