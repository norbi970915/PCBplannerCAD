// Dimension tool (P D): click the two measured points, then where the dimension
// line goes. Space cycles aligned / horizontal / vertical.

import { L, isInnerCopper } from "../../core/layers";
import { useEditor } from "../../core/store";
import type { DimensionItem, Item, Point } from "../../core/types";
import { dimensionLabel, dimensionThrough } from "../dimension";
import type { Tool, ToolEnv } from "./types";

export type DimStyle = "aligned" | "horizontal" | "vertical";
const NEXT: Record<DimStyle, DimStyle> = { aligned: "horizontal", horizontal: "vertical", vertical: "aligned" };
let style: DimStyle = "aligned";

const isCopper = (l: number) => l === L.topCopper || l === L.bottomCopper || isInnerCopper(l);

/** Dimension drawn on the active layer, or on Mechanical 1 when that is copper. */
function base(env: ToolEnv): Omit<DimensionItem, "a" | "b" | "offset" | "orthogonal" | "angle"> {
  const pl = env.placement();
  const units = useEditor.getState().units;
  const layer = isCopper(env.layer()) ? L.mech1 : env.layer();
  return { kind: "dimension", layer, width: pl.lineWidth, textHeight: Math.min(pl.textHeight, 1_000_000), units, precision: units === "mm" ? 2 : 1 };
}

export function dimensionTool(): Tool {
  let a: Point | null = null;
  let b: Point | null = null;

  const current = (env: ToolEnv): DimensionItem | null => {
    if (!a) return null;
    const c = env.cursor();
    if (!b) return c.x === a.x && c.y === a.y ? null : dimensionThrough(a, c, a, style, base(env));
    return dimensionThrough(a, b, c, style, base(env));
  };

  return {
    id: "dimension",
    hint: (env) => {
      const d = current(env);
      const head = `Dimension (${style}${isCopper(env.layer()) ? ", on Mechanical 1" : ""})`;
      if (!a) return `${head}: click the first point · Space: aligned / horizontal / vertical · Esc to finish`;
      if (!b) return `${head}: click the second point${d ? ` · ${dimensionLabel(d)}` : ""} · Space changes style · Esc cancels`;
      return `${head}: click where the dimension line goes${d ? ` · ${dimensionLabel(d)}` : ""} · Space changes style · Esc cancels`;
    },
    reset() {
      a = b = null;
    },
    preview(env): Item[] {
      const d = current(env);
      return d ? [d] : [];
    },
    down(env) {
      const c = env.cursor();
      if (!a) a = c;
      else if (!b) {
        if (c.x !== a.x || c.y !== a.y) b = c;
      } else {
        const d = current(env);
        a = b = null;
        if (d) void env.exec({ op: "add", items: [d] });
      }
    },
    key(env, key) {
      if (key !== " ") return false;
      style = NEXT[style];
      env.redraw();
      return true;
    },
    cancel() {
      if (!a) return false;
      a = b = null;
      return true;
    },
  };
}
