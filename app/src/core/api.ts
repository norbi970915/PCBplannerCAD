import { invoke } from "@tauri-apps/api/core";
import type {
  AirWire,
  CleanupOptions,
  CleanupResult,
  TeardropOptions,
  BoardModel3d,
  BoardSettings,
  DrcReport,
  DrcViolation,
  EcoChange,
  Mesh3d,
  Point,
  RouteRequest,
  RouteResult,
  TrackItem,
  RoutingStats,
  ViaItem,
  CachedPart,
  Command,
  Compiled,
  Component,
  ComponentEntry,
  DocInfo,
  DocKind,
  Field,
  Footprint,
  ItemId,
  LibInfo,
  Library,
  Model3d,
  ProjectState,
  ReportLine,
  Snapshot,
  Update,
  IdItem,
  Item,
  RouteMode,
  CornerStyle,
} from "./types";

export type EntryKind = "symbol" | "footprint" | "component";
export type LibrarySource = "structuredSymbols" | "structuredFootprints" | "xmlLibrary";
export type DesignSource = "structuredProject" | "structuredSchematic" | "xmlSchematic";

export interface SymbolMeta {
  name: string;
  description: string;
  prefix: string;
  showPinNumbers: boolean;
  showPinNames: boolean;
  pinNameOffset: number;
  isPower: boolean;
  fields: Field[];
}

export interface FootprintMeta {
  name: string;
  description: string;
  model: Model3d | null;
}

// Routing endpoints return Rust structs; document commands require tagged Item values.
type RouteResponse = Omit<RouteResult, "tracks"> & { tracks: Omit<TrackItem, "kind">[] };
const routeItems = (result: RouteResponse): RouteResult => ({
  ...result, tracks: result.tracks.map(track => ({ ...track, kind: "track" })),
});

export const api = {
  // documents
  docList: () => invoke<DocInfo[]>("doc_list"),
  snapshot: (id: string) => invoke<Snapshot>("doc_snapshot", { id }),
  newDocument: (kind: DocKind) => invoke<Snapshot>("doc_new", { kind }),
  open: (path: string) => invoke<Snapshot>("doc_open", { path }),
  execute: (id: string, cmd: Command) => invoke<Update>("doc_execute", { id, cmd }),
  undo: (id: string) => invoke<Update>("doc_undo", { id }),
  redo: (id: string) => invoke<Update>("doc_redo", { id }),
  save: (id: string, path: string | null) => invoke<DocInfo>("doc_save", { id, path }),
  close: (id: string) => invoke<void>("doc_close", { id }),
  putCache: (id: string, parts: CachedPart[]) => invoke<Record<string, CachedPart>>("doc_put_cache", { id, parts }),
  cacheComponent: (id: string, library: string, component: string) =>
    invoke<[string, CachedPart]>("doc_cache_component", { id, library, component }),

  // libraries
  libList: () => invoke<LibInfo[]>("lib_list"),
  libGet: (id: string) => invoke<Library>("lib_get", { id }),
  libNew: (name: string) => invoke<LibInfo>("lib_new", { name }),
  libOpen: (path: string) => invoke<LibInfo>("lib_open", { path }),
  libSave: (id: string, path: string | null) => invoke<LibInfo>("lib_save", { id, path }),
  libClose: (id: string) => invoke<string[]>("lib_close", { id }),
  componentIndex: () => invoke<ComponentEntry[]>("lib_component_index"),
  cachedPart: (library: string, component: string) => invoke<CachedPart>("lib_cached_part", { library, component }),
  findFootprint: (library: string, reference: string) => invoke<Footprint | null>("lib_find_footprint", { library, reference }),
  newSymbol: (id: string, name: string, prefix: string) => invoke<string>("lib_new_symbol", { id, name, prefix }),
  newFootprint: (id: string, name: string) => invoke<string>("lib_new_footprint", { id, name }),
  putComponent: (id: string, component: Component, previous: string | null) =>
    invoke<void>("lib_put_component", { id, component, previous }),
  updateSymbol: (id: string, name: string, meta: SymbolMeta) => invoke<void>("lib_update_symbol", { id, name, meta }),
  updateFootprint: (id: string, name: string, meta: FootprintMeta) => invoke<void>("lib_update_footprint", { id, name, meta }),
  deleteEntry: (id: string, kind: EntryKind, name: string) => invoke<string[]>("lib_delete_entry", { id, kind, name }),
  duplicateEntry: (id: string, kind: EntryKind, name: string) => invoke<string>("lib_duplicate_entry", { id, kind, name }),
  copyComponents: (from: string, to: string, components: string[]) => invoke<number>("lib_copy_components", { from, to, components }),
  openSymbol: (id: string, name: string) => invoke<DocInfo[]>("lib_open_symbol", { id, name }),
  addSymbolUnit: (id: string, name: string) => invoke<DocInfo[]>("lib_add_symbol_unit", { id, name }),
  removeSymbolUnit: (id: string, name: string, unit: number) => invoke<DocInfo[]>("lib_remove_symbol_unit", { id, name, unit }),
  openFootprint: (id: string, name: string) => invoke<DocInfo>("lib_open_footprint", { id, name }),

  // wizards
  wizardFamilies: () => invoke<WizardFamily[]>("wizard_families"),
  wizardDefaultName: (spec: WizardSpec) => invoke<string>("wizard_default_name", { spec }),
  wizardPreview: (spec: WizardSpec) => invoke<Footprint>("wizard_preview", { spec }),
  wizardGenerate: (id: string, spec: WizardSpec, name: string | null) => invoke<string>("wizard_generate", { id, spec, name }),
  symbolWizard: (id: string, spec: SymbolSpec) => invoke<string>("symbol_wizard", { id, spec }),

  // project
  projectInfo: () => invoke<ProjectState>("project_info"),
  projectOpen: (path: string) => invoke<ProjectState>("project_open", { path }),
  projectNew: (path: string) => invoke<ProjectState>("project_new", { path }),
  projectClose: () => invoke<ProjectState>("project_close"),
  projectAddSheet: (path: string) => invoke<ProjectState>("project_add_sheet", { path }),
  projectRemoveSheet: (file: string) => invoke<ProjectState>("project_remove_sheet", { file }),
  projectAddLibrary: (id: string) => invoke<ProjectState>("project_add_library", { id }),
  projectRemoveLibrary: (id: string) => invoke<ProjectState>("project_remove_library", { id }),
  projectSaveAll: () => invoke<DocInfo[]>("project_save_all"),
  compile: () => invoke<Compiled>("project_compile"),
  annotate: (all: boolean) => invoke<{ changed: number; docs: string[] }>("project_annotate", { all }),
  exportBom: (path: string) => invoke<number>("project_export_bom", { path }),
  connectedItems: (id: string, item: ItemId) => invoke<ItemId[]>("sheet_connected_items", { id, item }),
  sheetDoc: (file: string) => invoke<string | null>("project_sheet_doc", { file }),

  // board
  projectNewPcb: () => invoke<ProjectState>("project_new_pcb"),
  pcbAnalysis: (id: string) => invoke<{ airwires: AirWire[]; stats: RoutingStats }>("pcb_analysis", { id }),
  pcbDrc: (id: string) => invoke<DrcReport>("pcb_drc", { id }),
  pcbDrcNear: (id: string, items: ItemId[]) => invoke<DrcViolation[]>("pcb_drc_near", { id, items }),
  pcbRepour: (id: string, zone: ItemId | null) => invoke<Update>("pcb_repour", { id, zone }),
  /** Moving footprints with their tracks kept connected, as a `route` command plan. */
  pcbDragFootprints: (id: string, req: { ids: ItemId[]; delta: Point; angle?: number; flipped?: boolean; center?: Point; mode: "disconnect" | "free" | "preserveAngles"; routeMode: RouteMode; corner: CornerStyle }) =>
    invoke<{ replace: IdItem[]; delete: ItemId[]; add: Item[]; rerouted: number; blocked: number }>("pcb_drag_footprints", { id, req }),
  /** Cleanup Tracks & Vias plan; commit it as a `route` command. */
  /** Teardrop regeneration plan (replaces all teardrops); commit it as a `route` command. */
  pcbTeardrops: (id: string, opts: TeardropOptions) => invoke<{ delete: ItemId[]; add: Item[]; vias: number; pads: number }>("pcb_teardrops", { id, opts }),
  pcbCleanup: (id: string, opts: CleanupOptions) => invoke<CleanupResult>("pcb_cleanup", { id, opts }),
  /** Stitching vias for a filled pour (tagged as items); commit them as a `route` command. */
  pcbStitch: async (id: string, req: { zone: ItemId; pitch: number; stagger: boolean; bothSides: boolean }) => {
    const r = await invoke<{ vias: Omit<ViaItem, "kind">[]; candidates: number; rejected: number }>("pcb_stitch", { id, req });
    return { ...r, vias: r.vias.map((v): ViaItem => ({ ...v, kind: "via" })) };
  },
  /** Several footprints, each by its own offset, with tracks kept connected (Pack & Move), as a `route` command plan. */
  pcbDragEach: (id: string, req: { moves: { id: ItemId; delta: Point }[]; mode: "disconnect" | "free" | "preserveAngles"; routeMode: RouteMode; corner: CornerStyle }) =>
    invoke<{ replace: IdItem[]; delete: ItemId[]; add: Item[]; rerouted: number; blocked: number }>("pcb_drag_each", { id, req }),
  pcbRoute: async (id: string, req: RouteRequest): Promise<RouteResult> => routeItems(await invoke<RouteResponse>("pcb_route", { id, req })),
  pcbRoutePair: async (id: string, req: RouteRequest, negNet: string, gap: number): Promise<[RouteResult, RouteResult]> => {
    const [positive, negative] = await invoke<[RouteResponse, RouteResponse]>("pcb_route_pair", { id, req, negNet, gap });
    return [routeItems(positive), routeItems(negative)];
  },
  pcbTune: (id: string, req: { tracks: ItemId[]; targetLength: number; amplitude: number; spacing: number; style?: "trombone" | "accordion" }) =>
    invoke<Update>("pcb_tune", { id, req }),
  pcbNetLength: (id: string, net: string) => invoke<number>("pcb_net_length", { id, net }),
  /** A via for `net`; with `from`/`to` the board's matching via type (blind, buried, micro) is used, else a through via. */
  pcbVia: async (id: string, net: string, pos: Point, from: number | null = null, to: number | null = null): Promise<ViaItem> => ({
    ...await invoke<Omit<ViaItem, "kind">>("pcb_via", { id, net, pos, from, to }), kind: "via",
  }),
  pcbObjectAt: (id: string, pos: Point, layer: number) => invoke<[ItemId, string] | null>("pcb_object_at", { id, pos, layer }),
  pcbNetItems: (id: string, net: string) => invoke<ItemId[]>("pcb_net_items", { id, net }),
  pcbNets: (id: string) => invoke<string[]>("pcb_nets", { id }),
  pcbCacheFootprint: (id: string, library: string, reference: string) => invoke<[string, Footprint]>("pcb_cache_footprint", { id, library, reference }),
  pcbEcoPlan: (id: string) => invoke<EcoChange[]>("pcb_eco_plan", { id }),
  pcbEcoApply: (id: string, selected: number[]) => invoke<Update>("pcb_eco_apply", { id, selected }),
  pcbFootprints: (id: string) => invoke<Record<string, Footprint>>("pcb_footprints", { id }),
  pcbPanelize: (id: string, spec: { rows: number; cols: number; spacing: number; rail: number; vscore: boolean; fiducials: boolean }) =>
    invoke<Snapshot>("pcb_panelize", { id, spec }),
  projectSetVariants: (variants: { name: string; dnp: string[] }[]) => invoke<ProjectState>("project_set_variants", { variants }),
  pcbBoardSettings: (id: string) => invoke<[ItemId, BoardSettings]>("pcb_board_settings", { id }),

  // outputs
  outFab: (id: string, dir: string, options: FabOptions, variant: string | null) => invoke<string[]>("out_fab", { id, dir, options, variant }),
  outPnp: (id: string, path: string, format: "generic" | "jlc" | "coordinates", variant: string | null) => invoke<void>("out_pnp", { id, path, format, variant }),
  outBoardBom: (id: string, path: string, format: "generic" | "jlc", variant: string | null) => invoke<void>("out_board_bom", { id, path, format, variant }),
  outIpc2581: (id: string, path: string) => invoke<void>("out_ipc2581", { id, path }),
  outBoardModel: (id: string) => invoke<BoardModel3d>("out_board_model", { id }),
  outStep: (path: string, name: string, meshes: Mesh3d[]) => invoke<void>("out_step", { path, name, meshes }),
  readGerbers: (path: string) => invoke<GerberFile[]>("out_read_gerbers", { path }),
  writeFileBytes: (path: string, bytes: Uint8Array) => invoke<void>("write_file_bytes", { path, bytes: Array.from(bytes) }),
  readFileBytes: (path: string) => invoke<ArrayBuffer>("read_file_bytes", { path }),
  resolveModelPath: (path: string, base: string | null) => invoke<string | null>("resolve_model_path", { path, base }),

  // import
  importLibrary: (source: LibrarySource, path: string) =>
    invoke<{ library: LibInfo; report: ReportLine[] }>("import_library", { source, path }),
  importDesign: (source: DesignSource, path: string, destDir: string) =>
    invoke<{ project: ProjectState; report: ReportLine[]; files: string[] }>("import_design", { source, path, destDir }),
  libraryPaths: () => invoke<{ symbols: string | null; footprints: string | null }>("library_paths"),
  listLibraryFiles: (dir: string) => invoke<string[]>("list_library_files", { dir }),
};

/** Footprint wizard spec: `{ family: "...", ...params }` (see libgen::wizard). */
export type WizardSpec = { family: string } & Record<string, unknown>;

export interface WizardParam {
  /** JSON field in the spec object. */
  key: string;
  label: string;
  kind: "number" | "integer" | "bool" | "choice" | "text" | string;
  unit: string | null;
  default: unknown;
  choices: unknown[];
  min: number | null;
  max: number | null;
  /** `null` means "derive automatically". */
  optional: boolean;
}

export interface WizardFamily {
  id: string;
  name: string;
  description: string;
  params: WizardParam[];
  defaultSpec: WizardSpec;
}

export interface GerberOptions {
  tentVias: boolean;
  includeRefDes: boolean;
  includeValues: boolean;
  plotOutlineOnAll: boolean;
  precision: number;
  projectName: string;
}

export interface FabOptions {
  gerber: GerberOptions;
  pnp: "generic" | "jlc" | "coordinates" | null;
  bom: "generic" | "jlc" | null;
  zip: boolean;
}

export type GerberShape =
  | { stroke: { a: Point; b: Point; width: number } }
  | { arcStroke: { center: Point; radius: number; startAngle: number; sweep: number; width: number } }
  | { polygon: { points: Point[]; dark: boolean } }
  | { circle: { center: Point; radius: number; dark: boolean } };

export interface GerberFile {
  name: string;
  function: string | null;
  shapes: GerberShape[];
  warnings: string[];
  bounds: [Point, Point] | null;
}

export interface SymbolSpec {
  name: string;
  prefix: string;
  pins: { number: string; name: string; electrical: string; side: string; hidden?: boolean }[];
}
