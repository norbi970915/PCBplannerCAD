import { DRAW_ORDER, hexToRgba, layer } from "../core/layers";
import type { DocKind, LayerId } from "../core/types";
import type { LayerMode } from "../core/view";
import type { Role } from "./draw";
import type { Rgba } from "./renderer";

export interface Theme {
  background: Rgba;
  grid: { dot: Rgba; major: Rgba; axis: Rgba };
  cursor: Rgba;
  selectTint: Rgba;
  color(role: Role): Rgba;
  rank(role: Role): number;
}

const SCH_COLORS: Record<string, string> = {
  wire: "#4da3ff",
  bus: "#3570d4",
  body: "#e0634b",
  bodyFill: "#2a2320",
  bodyText: "#d7d9de",
  pin: "#c9ccd3",
  pinHidden: "#5d626c",
  pinName: "#c9ccd3",
  pinNum: "#8d95a3",
  designator: "#5ec4ff",
  value: "#b8bcc6",
  field: "#8e949e",
  label: "#ffcf5a",
  globalLabel: "#ffae42",
  hierLabel: "#ff9a4d",
  power: "#ff6b6b",
  noConnect: "#ff5555",
  sheet: "#d9973e",
  sheetFill: "#241e17",
  sheetName: "#f0c27a",
  sheetFile: "#a88b62",
  sheetEntry: "#f0c27a",
  text: "#d0d4dc",
  graphic: "#b69cff",
  graphicFill: "#2a2438",
  error: "#ff4d4d",
};

/** Schematic colors for the light UI theme. */
const SCH_LIGHT: Record<string, string> = {
  wire: "#1f5fbf",
  bus: "#1d3f8c",
  body: "#a3302a",
  bodyFill: "#fffbd6",
  bodyText: "#25282e",
  pin: "#25282e",
  pinHidden: "#9da3ae",
  pinName: "#25282e",
  pinNum: "#535b66",
  designator: "#1f4e8c",
  value: "#535b66",
  field: "#616875",
  label: "#7a4a00",
  globalLabel: "#a85500",
  hierLabel: "#a85500",
  power: "#b0281e",
  noConnect: "#d0302a",
  sheet: "#8a4b12",
  sheetFill: "#fdf3e1",
  sheetName: "#6b3a0c",
  sheetFile: "#8a6a3a",
  sheetEntry: "#6b3a0c",
  text: "#25282e",
  graphic: "#5b3fa8",
  graphicFill: "#efeaff",
  error: "#d0302a",
};

const BG_SCH: Rgba = [0.075, 0.082, 0.098, 1];
const BG_SCH_LIGHT: Rgba = [1, 1, 1, 1];
const BG_PCB: Rgba = [0, 0, 0, 1];

const lerp = (c: Rgba, t: Rgba, k: number): Rgba => [c[0] + (t[0] - c[0]) * k, c[1] + (t[1] - c[1]) * k, c[2] + (t[2] - c[2]) * k, c[3]];
export const mix = lerp;

const schCache = new Map<string, Rgba>();

export type UiTheme = "light" | "dark";

export function schematicTheme(ui: UiTheme = "dark"): Theme {
  const light = ui === "light";
  const palette = light ? SCH_LIGHT : SCH_COLORS;
  return {
    background: light ? BG_SCH_LIGHT : BG_SCH,
    grid: light
      ? { dot: [0.62, 0.65, 0.7, 0.9], major: [0.85, 0.87, 0.9, 0.0], axis: [0.78, 0.47, 0.18, 0.35] }
      : { dot: [0.36, 0.39, 0.45, 0.9], major: [0.2, 0.22, 0.26, 0.0], axis: [0.85, 0.55, 0.2, 0.25] },
    cursor: light ? [0.15, 0.16, 0.18, 0.9] : [0.95, 0.95, 0.95, 0.9],
    selectTint: light ? [0.22, 0.41, 0.62, 1] : [0.49, 0.95, 0.78, 1],
    color(role) {
      const key = `${ui}:${role}`;
      let c = schCache.get(key);
      if (!c) schCache.set(key, (c = hexToRgba(palette[role] ?? "#888888")));
      return c;
    },
    rank(role) {
      return role === "bodyFill" || role === "sheetFill" || role === "graphicFill" ? 0 : 1;
    },
  };
}

const HIDDEN: Rgba = [0, 0, 0, 0];
const isCopper = (id: LayerId) => id === 0 || id === 1 || (id >= 32 && id < 63);

/** Board/footprint theme; non-active layers are dimmed toward the background, or hidden in "only" mode. */
export function boardTheme(activeLayer: LayerId, mode: LayerMode): Theme {
  const order = new Map(DRAW_ORDER.map((l, i) => [l, i]));
  const cache = new Map<string, Rgba>();
  const singleLayer = mode !== "all";
  const only = mode === "only";
  return {
    background: BG_PCB,
    grid: { dot: [0.35, 0.39, 0.45, 0.5], major: [0.22, 0.26, 0.32, 0.32], axis: [0.65, 0.49, 0.3, 0.25] },
    cursor: [0.95, 0.95, 0.95, 0.9],
    selectTint: [1, 1, 1, 1],
    color(role) {
      let c = cache.get(role);
      if (c) return c;
      if (role === "hole") c = BG_PCB;
      else if (role === "via") c = only && !isCopper(activeLayer) ? HIDDEN : singleLayer ? lerp(hexToRgba("#a4b0bb"), BG_PCB, 0.4) : hexToRgba("#c2c4c8");
      else if (role === "keepout") c = hexToRgba("#ff4fc1", 0.9);
      else if (role === "ratsnest") c = [0.65, 0.73, 0.82, 0.5];
      else if (role === "drc") c = [0.2, 1, 0.35, 1];
      else if (role.startsWith("Z")) c = only && Number(role.slice(1)) !== activeLayer ? HIDDEN : lerp(hexToRgba(layer(Number(role.slice(1))).color), BG_PCB, Number(role.slice(1))===activeLayer ? 0.35 : 0.86);
      else if (role === "padNum") c = [0.9, 0.94, 0.98, 1];
      else if (role.startsWith("L")) {
        const id = Number(role.slice(1));
        const base = hexToRgba(layer(id).color);
        const active = id === activeLayer || (id === 15 && isCopper(activeLayer));
        c = only && !active ? HIDDEN : singleLayer && !active ? lerp(base, BG_PCB, 0.82) : active ? base : lerp(base, BG_PCB, 0.48);
      } else c = [0.8, 0.8, 0.8, 1];
      cache.set(role, c);
      return c;
    },
    rank(role) {
      if (role === "hole") return 900;
      if (role === "padNum") return 1000;
      if (role === "via") return 750;
      if (role === "ratsnest" || role === "drc") return 1100;
      if (role === "keepout") return 690;
      if (role.startsWith("Z")) {
        const zid = Number(role.slice(1));
        return zid === activeLayer ? 699 : (order.get(zid) ?? 100) - 0.1;
      }
      if (!role.startsWith("L")) return 800;
      const id = Number(role.slice(1));
      if (id === activeLayer) return 700;
      if (id === 15) return 710;
      // Inner copper paints between bottom and top copper.
      return order.get(id) ?? (id >= 32 && id < 63 ? (order.get(1) ?? 5) + 0.5 + (id - 32) / 100 : 100);
    },
  };
}

export function themeFor(kind: DocKind, activeLayer: LayerId, mode: LayerMode, ui: UiTheme = "dark"): Theme {
  return kind === "schematic" || kind === "symbol" ? schematicTheme(ui) : boardTheme(activeLayer, mode);
}
