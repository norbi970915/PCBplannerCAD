/** Display and interaction preferences; never part of design geometry or undo. */
export interface ViewSettings {
  hiddenLayers: readonly number[];
  references: boolean;
  values: boolean;
  padNumbers: boolean;
  padNetNames: boolean;
  text: boolean;
  tracks: boolean;
  vias: boolean;
  pads: boolean;
  graphics: boolean;
  airwires: boolean;
  drc: boolean;
  grid: boolean;
  snapToGrid: boolean;
}

export const DEFAULT_VIEW: ViewSettings = {
  hiddenLayers: [], references: true, values: true, padNumbers: true,
  padNetNames: false, text: true, tracks: true, vias: true, pads: true,
  graphics: true, airwires: true, drc: true, grid: true, snapToGrid: true,
};

export function normalizeView(value: unknown): ViewSettings {
  const out = { ...DEFAULT_VIEW, hiddenLayers: [] as number[] };
  if (!value || typeof value !== 'object') return out;
  const input = value as Record<string, unknown>;
  for (const key of Object.keys(DEFAULT_VIEW) as (keyof ViewSettings)[]) {
    if (key !== 'hiddenLayers' && typeof input[key] === 'boolean') out[key] = input[key] as boolean;
  }
  if (Array.isArray(input.hiddenLayers)) out.hiddenLayers = [...new Set(input.hiddenLayers.filter((x): x is number => Number.isSafeInteger(x) && x >= 0 && x < 100))];
  return out;
}

/** Shift+S cycle: every layer, other layers dimmed, active layer only. */
export type LayerMode = "all" | "dim" | "only";
export const LAYER_MODES: Record<LayerMode, { next: LayerMode; label: string }> = {
  all: { next: "dim", label: "All layers" },
  dim: { next: "only", label: "Other layers dimmed" },
  only: { next: "all", label: "Active layer only" },
};

export const layerVisible =(view: ViewSettings | undefined, layer: number) => !view?.hiddenLayers.includes(layer);
export const gridPoint = (p: { x: number; y: number }, grid: number) => ({ x: Math.round(p.x / grid) * grid, y: Math.round(p.y / grid) * grid });

/** Free movement still uses whole nanometers accepted by the native model. */
export const cursorPoint = (p: { x: number; y: number }, grid: number, snap: boolean) => snap ? gridPoint(p, grid) : { x: Math.round(p.x), y: Math.round(p.y) };
