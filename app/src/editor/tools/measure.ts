// Measure distance: click two points (snapping to pads, track ends and
// corners); shows the distance and ΔX / ΔY on the canvas until Esc.

import { useEditor } from "../../core/store";
import type { Point } from "../../core/types";
import { formatLength } from "../../core/units";
import { textSegments } from "../font";
import type { DrawLine, Rgba } from "../renderer";
import { ringHandles } from "./polygon";
import type { Tool, ToolEnv } from "./types";

const LINE: Rgba = [1, 0.82, 0.25, 1];
const AXIS: Rgba = [1, 0.82, 0.25, 0.45];
const TEXT: Rgba = [1, 1, 1, 1];

function label(a: Point, b: Point): string[] {
  const u = useEditor.getState().units;
  const dx = b.x - a.x;
  const dy = b.y - a.y;
  const ang = (Math.atan2(dy, dx) * 180) / Math.PI;
  // ASCII only: the canvas stroke font has no Greek or math glyphs.
  return [`${formatLength(Math.round(Math.hypot(dx, dy)), u)}  ${ang.toFixed(1)}°`, `dX ${formatLength(Math.abs(dx), u)}  dY ${formatLength(Math.abs(dy), u)}`];
}

export function measureTool(): Tool {
  let a: Point | null = null;
  let b: Point | null = null;

  const ends = (env: ToolEnv): [Point, Point] | null => (a ? [a, b ?? env.cursor()] : null);

  return {
    id: "measure",
    hint: (env) => {
      const e = ends(env);
      if (!e) return "Measure: click the first point (snaps to pads, track ends and corners)";
      const [l1, l2] = label(e[0], e[1]);
      return `${l1} · ${l2} · ${b ? "click to measure again" : "click the second point"} · Esc to finish`;
    },
    reset() {
      a = b = null;
    },
    down(env) {
      const c = env.cursor();
      if (!a || b) {
        a = c;
        b = null;
      } else b = c;
    },
    cancel() {
      if (!a) return false;
      a = b = null;
      return true;
    },
    overlay(env) {
      const e = ends(env);
      if (!e) return [];
      const [p, q] = e;
      const px = 1 / env.scale();
      const out: DrawLine[] = [
        { a: p, b: { x: q.x, y: p.y }, width: px, color: AXIS },
        { a: { x: q.x, y: p.y }, b: q, width: px, color: AXIS },
        { a: p, b: q, width: 1.5 * px, color: LINE },
        ...ringHandles([p, q], px, false),
      ];
      // Readout next to the end point, sized in screen pixels.
      const h = 11 * px;
      label(p, q).forEach((text, i) => {
        const at = { x: q.x + 12 * px, y: q.y - 12 * px - i * h * 1.6 };
        for (const [s, t] of textSegments(text, at, { height: h, rotation: 0, hAlign: "left", vAlign: "top", mirrored: false })) {
          out.push({ a: s, b: t, width: 1.2 * px, color: TEXT });
        }
      });
      return out;
    },
  };
}
