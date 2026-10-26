import type { Item, LayerId } from "./types";

// Mirrors crates/core/src/layer.rs.
export const L = {
  topCopper: 0,
  bottomCopper: 1,
  topSilk: 2,
  bottomSilk: 3,
  mech1: 4,
  keepOut: 5,
  topMask: 6,
  bottomMask: 7,
  topPaste: 8,
  bottomPaste: 9,
  topCourtyard: 10,
  bottomCourtyard: 11,
  topAssembly: 12,
  bottomAssembly: 13,
  boardOutline: 14,
  multi: 15,
  schematic: 100,
} as const;

export interface LayerDef {
  id: LayerId;
  name: string;
  short: string;
  color: string;
}

export const LAYERS: LayerDef[] = [
  { id: L.topCopper, name: "Top Layer", short: "T.Cu", color: "#e8463a" },
  { id: L.bottomCopper, name: "Bottom Layer", short: "B.Cu", color: "#3b6ff5" },
  { id: L.topSilk, name: "Top Overlay", short: "T.Silk", color: "#f2d24b" },
  { id: L.bottomSilk, name: "Bottom Overlay", short: "B.Silk", color: "#a07a3a" },
  { id: L.topMask, name: "Top Solder", short: "T.Mask", color: "#a2457e" },
  { id: L.bottomMask, name: "Bottom Solder", short: "B.Mask", color: "#7b4fa8" },
  { id: L.topPaste, name: "Top Paste", short: "T.Paste", color: "#8f9399" },
  { id: L.bottomPaste, name: "Bottom Paste", short: "B.Paste", color: "#7a5a5a" },
  { id: L.topCourtyard, name: "Top Courtyard", short: "T.Crt", color: "#3fc4c4" },
  { id: L.bottomCourtyard, name: "Bottom Courtyard", short: "B.Crt", color: "#2a8a8a" },
  { id: L.topAssembly, name: "Top Assembly", short: "T.Asm", color: "#b6bac2" },
  { id: L.bottomAssembly, name: "Bottom Assembly", short: "B.Asm", color: "#80858d" },
  { id: L.mech1, name: "Mechanical 1", short: "Mech1", color: "#c46be0" },
  { id: L.boardOutline, name: "Board Outline", short: "Edge", color: "#e6c84a" },
  { id: L.keepOut, name: "Keep-Out", short: "Keep", color: "#ff4fc1" },
  { id: L.multi, name: "Multi-Layer", short: "Multi", color: "#c2c4c8" },
];

/** Paint order, bottom-up (the active layer is always painted last). */
export const DRAW_ORDER: LayerId[] = [
  L.bottomAssembly,
  L.bottomCourtyard,
  L.bottomPaste,
  L.bottomMask,
  L.bottomSilk,
  L.bottomCopper,
  L.topCopper,
  L.multi,
  L.topMask,
  L.topPaste,
  L.topSilk,
  L.topCourtyard,
  L.topAssembly,
  L.mech1,
  L.boardOutline,
  L.keepOut,
];

/** Layer tabs shown per editor. */
export const FOOTPRINT_TABS: LayerId[] = [
  L.topCopper,
  L.bottomCopper,
  L.topSilk,
  L.bottomSilk,
  L.topMask,
  L.topPaste,
  L.topCourtyard,
  L.topAssembly,
  L.mech1,
  L.boardOutline,
];

const byId = new Map(LAYERS.map((l) => [l.id, l]));

/** Inner copper colors, by inner layer index. */
const INNER_COLORS = ["#d8a83a", "#3fb56c", "#c45bc4", "#2fb0c0", "#e07a35", "#7f7fe0", "#b8c040", "#e0608a", "#40a0a0", "#a07850", "#6090e0", "#c0c0c0"];

export const isInnerCopper = (id: LayerId) => id >= 32 && id < 63;

export function layer(id: LayerId): LayerDef {
  const known = byId.get(id);
  if (known) return known;
  if (isInnerCopper(id)) return { id, name: `Inner ${id - 32}`, short: `In${id - 32}`, color: INNER_COLORS[(id - 33 + INNER_COLORS.length) % INNER_COLORS.length] };
  return { id, name: `Layer ${id}`, short: `L${id}`, color: "#888888" };
}

/** Copper layers of a board in stack order (top → inner → bottom), with stack names. */
export function boardCopper(items: Iterable<Item>): { id: LayerId; name: string }[] {
  for (const it of items) {
    if (it.kind !== "board") continue;
    const cu = it.stackup.layers.filter((l) => l.kind === "copper" && l.layer !== null).map((l) => ({ id: l.layer as LayerId, name: l.name }));
    if (cu.length) return cu;
  }
  return [
    { id: L.topCopper, name: layer(L.topCopper).name },
    { id: L.bottomCopper, name: layer(L.bottomCopper).name },
  ];
}

/** Layer tabs of a board: every copper layer of its stack, then the rest. */
export function boardTabs(items: Iterable<Item>): { id: LayerId; name: string }[] {
  const rest = [L.topSilk, L.bottomSilk, L.topMask, L.bottomMask, L.topPaste, L.bottomPaste, L.mech1, L.boardOutline, L.keepOut];
  return [...boardCopper(items), ...rest.map((id) => ({ id, name: layer(id).name }))];
}

export function hexToRgba(hex: string, alpha = 1): [number, number, number, number] {
  const n = parseInt(hex.slice(1), 16);
  return [((n >> 16) & 255) / 255, ((n >> 8) & 255) / 255, (n & 255) / 255, alpha];
}
