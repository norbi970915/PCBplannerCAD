import { create } from "zustand";
import { readPreference } from "./preferences";
import { L } from "./layers";
import type {
  AirWire,
  CachedPart,
  DrcViolation,
  Footprint,
  Compiled,
  Coord,
  DocInfo,
  DocKind,
  FillStyle,
  Item,
  ItemId,
  LabelScope,
  LayerId,
  LibInfo,
  PadKind,
  PadShape,
  PinShape,
  PinType,
  Point,
  PortShape,
  PowerStyle,
  ProjectInfo,
  Snapshot,
  Update,
} from "./types";
import { fromUnits, GRID_PRESETS, NM_PER_MIL, type Units } from "./units";

export interface DocState {
  info: DocInfo;
  items: ReadonlyMap<ItemId, Item>;
  cache: Record<string, CachedPart>;
  /** PCB: footprint geometry keyed like PlacedFootprint.footprint. */
  footprints: Record<string, Footprint>;
  /** PCB: unrouted connections and the latest DRC result. */
  airwires: AirWire[];
  drc: DrcViolation[];
  selection: ReadonlySet<ItemId>;
  /** Highlighted items (net highlight, ERC navigation). */
  marked: ReadonlySet<ItemId>;
  tool: string;
  activeLayer: LayerId;
  singleLayer: boolean;
  grid: Coord;
  /** Bumped when the whole document is (re)loaded, so views refit. */
  epoch: number;
  /** A point the view should center on (ERC navigation). */
  focus: { pos: Point; seq: number } | null;
}

/** Parameters for placement tools (edited in the Properties panel). */
export interface Placement {
  rotation: number;
  mirror: boolean;
  lineWidth: Coord;
  textHeight: Coord;
  text: string;
  fill: FillStyle;
  labelText: string;
  labelScope: LabelScope;
  labelShape: PortShape;
  powerNet: string;
  powerStyle: PowerStyle;
  pinName: string;
  pinNumber: string;
  pinLength: Coord;
  pinType: PinType;
  pinShape: PinShape;
  padNumber: string;
  padShape: PadShape;
  padKind: PadKind;
  padWidth: Coord;
  padHeight: Coord;
  padDrill: Coord;
  padCornerRatio: number;
  sheetName: string;
  part: { library: string; component: string; key: string; unit: number; designator: string | null } | null;
  /** PCB: net for new zones/tracks when not picked from copper; route options. */
  net: string;
  routeMode: "ignore" | "walkaround" | "shove";
  cornerStyle: "diagonal45" | "orthogonal90" | "any";
  trackWidth: Coord | null;
  zonePriority: number;
  /** Footprint being placed on a board. */
  footprint: { key: string; designator: string; value: string; component: string } | null;
  /** Items being pasted, positioned relative to `anchor`. */
  paste: { items: Item[]; anchor: Point } | null;
}

export interface MessageTarget {
  docId?: string;
  sheet?: string;
  items?: ItemId[];
  pos?: Point | null;
}

export interface Message {
  id: number;
  time: Date;
  level: "info" | "warning" | "error";
  text: string;
  source?: string;
  target?: MessageTarget;
}

const mm = (v: number) => fromUnits(v, "mm");

const DEFAULT_GRID: Record<DocKind, Coord> = {
  schematic: mm(1.27),
  symbol: mm(1.27),
  footprint: mm(0.05),
  pcb: mm(0.1),
};

const DEFAULT_PLACEMENT: Placement = {
  rotation: 0,
  mirror: false,
  lineWidth: mm(0.15),
  textHeight: mm(1.27),
  text: "Text",
  fill: "none",
  labelText: "NET1",
  labelScope: "local",
  labelShape: "passive",
  powerNet: "VCC",
  powerStyle: "bar",
  pinName: "PIN",
  pinNumber: "1",
  pinLength: mm(2.54),
  pinType: "passive",
  pinShape: "line",
  padNumber: "1",
  padShape: "roundRect",
  padKind: "smd",
  padWidth: mm(1.0),
  padHeight: mm(1.2),
  padDrill: mm(0.8),
  padCornerRatio: 0.25,
  sheetName: "Sheet",
  part: null,
  paste: null,
  net: "",
  routeMode: "walkaround",
  cornerStyle: "diagonal45",
  trackWidth: null,
  zonePriority: 0,
  footprint: null,
};

let messageSeq = 0;

interface AppState {
  docs: Record<string, DocState>;
  activeDocId: string | null;
  libs: LibInfo[];
  /** Bumped per library whenever its content changes; views refetch on change. */
  libRevision: Record<string, number>;
  project: ProjectInfo | null;
  compiled: Compiled | null;
  units: Units;
  /** Draw pour fills; off shows pours as outlines only (a view setting). */
  showPourFills: boolean;
  cursor: Point | null;
  pendingKeys: string | null;
  hint: string;
  messages: Message[];
  placement: Placement;
  /** Component highlighted in the Components panel. */
  pickedComponent: { library: string; component: string } | null;

  loadDoc: (s: Snapshot) => void;
  applyUpdate: (u: Update) => void;
  setInfo: (info: DocInfo) => void;
  removeDoc: (id: string) => void;
  patchDoc: (id: string, patch: Partial<DocState>) => void;
  setActiveDoc: (id: string | null) => void;
  setCache: (id: string, key: string, part: CachedPart) => void;
  setFootprint: (id: string, key: string, fp: Footprint) => void;
  setLibs: (libs: LibInfo[]) => void;
  bumpLib: (id: string) => void;
  setProject: (p: ProjectInfo | null) => void;
  setCompiled: (c: Compiled | null) => void;
  toggleUnits: () => void;
  togglePourFills: () => void;
  cycleGrid: (step: 1 | -1) => void;
  setCursor: (p: Point | null) => void;
  setPendingKeys: (k: string | null) => void;
  setHint: (h: string) => void;
  setPlacement: (patch: Partial<Placement>) => void;
  pickComponent: (c: { library: string; component: string } | null) => void;
  log: (level: Message["level"], text: string, extra?: { source?: string; target?: MessageTarget }) => void;
  clearMessages: (source?: string) => void;
}

const FILLS_KEY = "pcbplannercad.pourFills";

function readFlag(key: string, fallback: boolean): boolean {
  try {
    const v = readPreference(key);
    return v === null ? fallback : v === "1";
  } catch {
    return fallback;
  }
}

function writeFlag(key: string, value: boolean) {
  try {
    localStorage.setItem(key, value ? "1" : "0");
  } catch {
    // A view preference; losing it is harmless.
  }
}

export const useEditor = create<AppState>()((set, get) => ({
  docs: {},
  activeDocId: null,
  libs: [],
  libRevision: {},
  project: null,
  compiled: null,
  units: "mm",
  showPourFills: readFlag(FILLS_KEY, true),
  cursor: null,
  pendingKeys: null,
  hint: "",
  messages: [],
  placement: DEFAULT_PLACEMENT,
  pickedComponent: null,

  loadDoc: (s) => {
    const prev = get().docs[s.info.id];
    const kind = s.info.kind;
    const doc: DocState = {
      info: s.info,
      items: new Map(s.items.map((u) => [u.id, u.item])),
      cache: s.cache,
      footprints: s.footprints ?? {},
      airwires: prev?.airwires ?? [],
      drc: prev?.drc ?? [],
      selection: new Set(),
      marked: new Set(),
      tool: prev?.tool ?? "select",
      activeLayer: prev?.activeLayer ?? (kind === "schematic" || kind === "symbol" ? L.schematic : L.topCopper),
      singleLayer: prev?.singleLayer ?? false,
      grid: prev?.grid ?? DEFAULT_GRID[kind],
      epoch: (prev?.epoch ?? 0) + 1,
      focus: null,
    };
    set({ docs: { ...get().docs, [s.info.id]: doc } });
  },

  applyUpdate: ({ delta, info }) => {
    const doc = get().docs[info.id];
    if (!doc) return;
    const items = new Map(doc.items);
    for (const id of delta.removed) items.delete(id);
    for (const u of delta.upserted) items.set(u.id, u.item);
    const selection = new Set([...doc.selection].filter((id) => items.has(id)));
    set({ docs: { ...get().docs, [info.id]: { ...doc, items, info, selection } } });
  },

  setInfo: (info) => {
    const doc = get().docs[info.id];
    if (doc) set({ docs: { ...get().docs, [info.id]: { ...doc, info } } });
  },

  removeDoc: (id) => {
    const docs = { ...get().docs };
    delete docs[id];
    set({ docs, activeDocId: get().activeDocId === id ? null : get().activeDocId });
  },

  patchDoc: (id, patch) => {
    const doc = get().docs[id];
    if (doc) set({ docs: { ...get().docs, [id]: { ...doc, ...patch } } });
  },

  setActiveDoc: (activeDocId) => set({ activeDocId }),

  setCache: (id, key, part) => {
    const doc = get().docs[id];
    if (doc) set({ docs: { ...get().docs, [id]: { ...doc, cache: { ...doc.cache, [key]: part } } } });
  },

  setFootprint: (id, key, fp) => {
    const doc = get().docs[id];
    if (doc) set({ docs: { ...get().docs, [id]: { ...doc, footprints: { ...doc.footprints, [key]: fp } } } });
  },

  setLibs: (libs) => set({ libs }),
  bumpLib: (id) => set({ libRevision: { ...get().libRevision, [id]: (get().libRevision[id] ?? 0) + 1 } }),
  setProject: (project) => set({ project }),
  setCompiled: (compiled) => set({ compiled }),

  toggleUnits: () => set({ units: get().units === "mm" ? "mil" : "mm" }),
  togglePourFills: () => {
    const showPourFills = !get().showPourFills;
    writeFlag(FILLS_KEY, showPourFills);
    set({ showPourFills });
  },

  cycleGrid: (step) => {
    const id = get().activeDocId;
    const doc = id ? get().docs[id] : undefined;
    if (!doc || !id) return;
    const presets = [...new Set([...GRID_PRESETS.mm, ...GRID_PRESETS.mil, mm(1.27), mm(2.54)])].sort((a, b) => a - b);
    const i = presets.findIndex((g) => g >= doc.grid);
    const next = presets[Math.min(presets.length - 1, Math.max(0, (i < 0 ? presets.length - 1 : i) + step))];
    get().patchDoc(id, { grid: next });
  },

  setCursor: (cursor) => set({ cursor }),
  setPendingKeys: (pendingKeys) => set({ pendingKeys }),
  setHint: (hint) => set({ hint }),
  setPlacement: (patch) => set({ placement: { ...get().placement, ...patch } }),
  pickComponent: (pickedComponent) => set({ pickedComponent }),

  log: (level, text, extra) =>
    set({
      messages: [...get().messages, { id: ++messageSeq, time: new Date(), level, text, ...extra }].slice(-2000),
    }),
  clearMessages: (source) => set({ messages: source ? get().messages.filter((m) => m.source !== source) : [] }),
}));

export const activeDoc = (): DocState | undefined => {
  const s = useEditor.getState();
  return s.activeDocId ? s.docs[s.activeDocId] : undefined;
};

export const DEFAULT_LINE_WIDTH_MIL = 10 * NM_PER_MIL;
