// Board selection filter: which object types clicks and box selection pick.

import { create } from "zustand";
import { readPreference } from "./preferences";
import type { DocKind, Item } from "./types";

export const FILTER_KINDS = [
  ["components", "Components"],
  ["tracks", "Tracks & arcs"],
  ["vias", "Vias"],
  ["pours", "Pours"],
  ["keepouts", "Keep-outs"],
  ["graphics", "Lines, shapes & outline"],
  ["text", "Text"],
] as const;

export type FilterKind = (typeof FILTER_KINDS)[number][0];

const KEY = "pcbplannercad.selectFilter";
const ALL: Record<FilterKind, boolean> = { components: true, tracks: true, vias: true, pours: true, keepouts: true, graphics: true, text: true };

function load(): Record<FilterKind, boolean> {
  try {
    return { ...ALL, ...JSON.parse(readPreference(KEY) ?? "{}") };
  } catch {
    return { ...ALL };
  }
}

interface FilterState {
  on: Record<FilterKind, boolean>;
  set: (k: FilterKind, v: boolean) => void;
  all: () => void;
  none: () => void;
  only: (k: FilterKind) => void;
}

export const useSelectFilter = create<FilterState>()((set, get) => ({
  on: load(),
  set: (k, v) => {
    const on = { ...get().on, [k]: v };
    try {
      localStorage.setItem(KEY, JSON.stringify(on));
    } catch {
      // A view preference; losing it is harmless.
    }
    set({ on });
  },
  none: () => {
    const on = Object.fromEntries(FILTER_KINDS.map(([k]) => [k, false])) as Record<FilterKind, boolean>;
    try { localStorage.setItem(KEY, JSON.stringify(on)); } catch { /* Optional preference. */ }
    set({ on });
  },
  only: (k) => {
    const on = Object.fromEntries(FILTER_KINDS.map(([key]) => [key, key === k])) as Record<FilterKind, boolean>;
    try { localStorage.setItem(KEY, JSON.stringify(on)); } catch { /* Optional preference. */ }
    set({ on });
  },
  all: () => {
    try {
      localStorage.removeItem(KEY);
    } catch {
      // ignore
    }
    set({ on: { ...ALL } });
  },
}));

export function filterKindOf(item: Item): FilterKind | null {
  switch (item.kind) {
    case "footprint":
      return "components";
    case "track":
    case "arcTrack":
      return "tracks";
    case "via":
      return "vias";
    case "zone":
      return item.keepout ? "keepouts" : "pours";
    case "text":
      return "text";
    case "line":
    case "arc":
    case "circle":
    case "rect":
    case "polyline":
      return "graphics";
    default:
      return null;
  }
}

/** True when the item may be picked in the given editor (filter applies to boards only). */
export function selectable(kind: DocKind, item: Item): boolean {
  if (kind !== "pcb") return true;
  const k = filterKindOf(item);
  return !k || useSelectFilter.getState().on[k];
}

export const filterActive = (on: Record<FilterKind, boolean>) => Object.values(on).some((v) => !v);
