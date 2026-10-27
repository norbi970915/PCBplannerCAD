// Board selection filter: which object types and layers clicks and box selection pick.
// An object is selectable when its type is enabled and it lies on an enabled layer.

import { create } from "zustand";
import { L } from "./layers";
import { readPreference } from "./preferences";
import { activeDoc, type DocState } from "./store";
import type { DocKind, Item, LayerId } from "./types";

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
const LOCKED_KEY = "pcbplannercad.selectFilterLocked";
const LAYERS_KEY = "pcbplannercad.selectFilterLayers";
const ALL: Record<FilterKind, boolean> = { components: true, tracks: true, vias: true, pours: true, keepouts: true, graphics: true, text: true };

function load(): Record<FilterKind, boolean> {
  try {
    return { ...ALL, ...JSON.parse(readPreference(KEY) ?? "{}") };
  } catch {
    return { ...ALL };
  }
}

/** Excluded layers are stored, so layers added later start selectable. */
function loadLayers(): LayerId[] {
  try {
    const v = JSON.parse(readPreference(LAYERS_KEY) ?? "[]");
    return Array.isArray(v) ? v.filter((x): x is LayerId => typeof x === "number") : [];
  } catch {
    return [];
  }
}

function save(key: string, value: unknown) {
  try {
    localStorage.setItem(key, JSON.stringify(value));
  } catch {
    // A view preference; losing it is harmless.
  }
}

interface FilterState {
  on: Record<FilterKind, boolean>;
  offLayers: LayerId[];
  excludeLocked: boolean;
  setExcludeLocked: (exclude:boolean) => void;
  set: (k: FilterKind, v: boolean) => void;
  all: () => void;
  none: () => void;
  only: (k: FilterKind) => void;
  /** Enable a preset of object types without changing layer exclusions. */
  onlyTypes: (kinds: readonly FilterKind[]) => void;
  setLayer: (id: LayerId, v: boolean) => void;
  /** Enables exactly `ids` out of `known`. */
  onlyLayers: (ids: LayerId[], known: LayerId[]) => void;
  allLayers: () => void;
  /** All types on all layers. */
  reset: () => void;
}

const setTypes = (on: Record<FilterKind, boolean>) => {
  save(KEY, on);
  return { on };
};
const setLayers = (offLayers: LayerId[]) => {
  save(LAYERS_KEY, offLayers);
  return { offLayers };
};

export const useSelectFilter = create<FilterState>()((set, get) => ({
  on: load(),
  offLayers: loadLayers(),
  excludeLocked: (()=>{try{return readPreference(LOCKED_KEY)==="true";}catch{return false;}})(),
  setExcludeLocked: (excludeLocked) => { save(LOCKED_KEY,excludeLocked); set({excludeLocked}); },
  set: (k, v) => set(setTypes({ ...get().on, [k]: v })),
  all: () => set(setTypes({ ...ALL })),
  none: () => set(setTypes(Object.fromEntries(FILTER_KINDS.map(([k]) => [k, false])) as Record<FilterKind, boolean>)),
  only: (k) => set(setTypes(Object.fromEntries(FILTER_KINDS.map(([key]) => [key, key === k])) as Record<FilterKind, boolean>)),
  onlyTypes: (kinds) => set(setTypes(Object.fromEntries(FILTER_KINDS.map(([key]) => [key, kinds.includes(key)])) as Record<FilterKind, boolean>)),
  setLayer: (id, v) => {
    const off = get().offLayers.filter((x) => x !== id);
    set(setLayers(v ? off : [...off, id]));
  },
  onlyLayers: (ids, known) => set(setLayers(known.filter((id) => !ids.includes(id)))),
  allLayers: () => set(setLayers([])),
  reset: () => { save(LOCKED_KEY,false); set({ ...setTypes({ ...ALL }), ...setLayers([]), excludeLocked:false }); },
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
    case "dimension":
      return "graphics";
    default:
      return null;
  }
}

/** Copper order: top, inner layers by number, bottom. */
const copperRank = (id: LayerId) => (id === L.topCopper ? 0 : id === L.bottomCopper ? 1000 : id - 32);

/** True when the item touches at least one layer the filter allows. */
export function onSelectableLayer(item: Item, offLayers: readonly LayerId[]): boolean {
  if (!offLayers.length) return true;
  const off = new Set(offLayers);
  const anyCopper = (lo: number, hi: number) => {
    for (const id of [L.topCopper, L.bottomCopper, ...Array.from({ length: 30 }, (_, i) => 33 + i)]) {
      const r = copperRank(id);
      if (r >= lo && r <= hi && !off.has(id)) return true;
    }
    return false;
  };
  if (item.kind === "via") {
    const [a, b] = [copperRank(item.fromLayer), copperRank(item.toLayer)];
    return anyCopper(Math.min(a, b), Math.max(a, b));
  }
  // Components belong to the copper layer of their side.
  if (item.kind === "footprint") return !off.has(item.side === "bottom" ? L.bottomCopper : L.topCopper);
  if ("layer" in item && typeof item.layer === "number") {
    if (item.layer === L.multi) return anyCopper(0, 1000);
    return !off.has(item.layer);
  }
  return true;
}

const ALL_LAYERS: LayerId[] = Array.from({ length: 63 }, (_, i) => i);

/** True when the item may be picked in the given editor (filter applies to boards only). */
export function selectable(kind: DocKind, item: Item, targetDoc:DocState|undefined = activeDoc()): boolean {
  if (kind !== "pcb") return true;
  // Teardrops belong to their joints; Tools ▸ Teardrops manages them.
  if (item.kind === "zone" && item.teardrop) return false;
  const { on, offLayers, excludeLocked } = useSelectFilter.getState();
  if (excludeLocked && "locked" in item && item.locked) return false;
  const k = filterKindOf(item);
  if ((k && !on[k]) || !onSelectableLayer(item, offLayers)) return false;
  // "Active layer only" hides the other layers, so their objects cannot be picked either.
  const doc = targetDoc;
  if (!onSelectableLayer(item, doc?.view?.hiddenLayers ?? [])) return false;
  if (doc?.view) {
    if ((item.kind === 'track' || item.kind === 'arcTrack') && !doc.view.tracks) return false;
    if (item.kind === 'via' && !doc.view.vias) return false;
    if (item.kind === 'text' && !doc.view.text) return false;
    if (filterKindOf(item) === 'graphics' && !doc.view.graphics) return false;
  }
  return doc?.layerMode !== "only" || onSelectableLayer(item, ALL_LAYERS.filter((id) => id !== doc.activeLayer));
}

export const typesFiltered = (on: Record<FilterKind, boolean>) => Object.values(on).some((v) => !v);
export const filterActive = (s: Pick<FilterState, "on" | "offLayers"> & {excludeLocked?:boolean}) => typesFiltered(s.on) || s.offLayers.length > 0 || s.excludeLocked === true;
