import { invoke } from "@tauri-apps/api/core";
import type {
  AirWire,
  IdItem,
  CleanupOptions,
  CleanupResult,
  TeardropOptions,
  BoardModel3d,
  BoardSettings,
  DrcReport,
  DrcApproval,
  DrcViolation,
  EcoChange,
  Mesh3d,
  Point,
  PcbDragPlan,
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
  Item,
  RouteMode,
  CornerStyle,
} from "./types";

import type { LengthConstraint } from '../editor/routeLengths';
export type TuneRequest = { tracks: ItemId[]; targetLength: number; amplitude: number; spacing: number; style?: "trombone" | "accordion" };
export type EntryKind = "symbol" | "footprint" | "component";
export type LibrarySource = "structuredSymbols" | "structuredFootprints" | "xmlLibrary";
export type DesignSource = "structuredProject" | "structuredSchematic" | "xmlSchematic";

export interface SavedPreview { views:{label:string;kind:DocKind;items:Item[];cache:Record<string,CachedPart>;footprints:Record<string,Footprint>;symbol:import('./types').LibSymbol|null}[];model:string|null;dependencies:string[];notes:string[] }
export interface HubEntry { name:string;path:string;kind:'folder'|'project'|'schematic'|'board'|'library'|'model'|'file';modifiedMs:number;bytes:number }
export interface RecoveryInfo {id:string;createdMs:number;title:string;documents:number;libraries:number;bytes:number;notes:string[]}
export interface RecoveryListing {location:string;entries:RecoveryInfo[];issues:string[]}
export interface RecoveryRestored {folder:string;project:string|null;files:string[];notes:string[]}
export interface RevisionInfo { id:string;createdMs:number;label:string;files:number;bytes:number }
export interface RevisionChanges { from:RevisionInfo;to:RevisionInfo;added:number;removed:number;modified:number;files:{path:string;status:'added'|'removed'|'modified';summary:string;details:string[];omitted:number}[] }
export interface ImportRequest { path:string;boardPath?:string|null;models:{footprint:string;path:string}[] }
export interface ImportReview { token:string;summary:{name:string;kind:'design'|'library';sheets:number;boardItems:number;boardFile:string|null;linkedDesign:boolean;symbols:number;footprints:string[];components:number;models:number;outputs:string[];report:ReportLine[]} }

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
  libSetNotices: (id: string, notices: string[]) => invoke<void>("lib_set_notices", { id, notices }),
  legalInfo: () => invoke<{ legal: string; data: string; migration: string[] }>("legal_info"),
  openLegalFolder: () => invoke<void>("open_legal_folder"),
  dockCloseWindow: (label:string)=>invoke<void>("dock_close_window",{label}),
  dockFocusWindow: (label:string)=>invoke<void>("dock_focus_window",{label}),
  // documents
  docList: () => invoke<DocInfo[]>("doc_list"),
  snapshot: (id: string) => invoke<Snapshot>("doc_snapshot", { id }),
  newDocument: (kind: DocKind) => invoke<Snapshot>("doc_new", { kind }),
  open: (path: string) => invoke<Snapshot>("doc_open", { path }),
  execute: (id: string, cmd: Command, enforceVias = true) => invoke<Update>("doc_execute", { id, cmd, enforceVias }),
  undo: (id: string) => invoke<Update>("doc_undo", { id }),
  redo: (id: string) => invoke<Update>("doc_redo", { id }),
  save: (id: string, path: string | null) => invoke<DocInfo>("doc_save", { id, path }),
  saveCopy: (id:string,path:string)=>invoke<void>("doc_save_copy",{id,path}),
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
  wizardPreview: (spec: WizardSpec, goals?: WizardGoals) => invoke<Footprint>("wizard_preview", { spec, goals: goals ?? null }),
  wizardGenerate: (id: string, spec: WizardSpec, name: string | null, goals?: WizardGoals) => invoke<string>("wizard_generate", { id, spec, name, goals: goals ?? null }),
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
  pcbRoutingDefaults: (settings: BoardSettings, net: string, layer: number) => invoke<{width:number;pairWidth:number;pairGap:number}>("pcb_routing_defaults", { settings, net, layer }),
  pcbValidateSetup: (settings: BoardSettings) => invoke<string[]>("pcb_validate_setup", { settings }),
  pcbDrcApproval: (id: string, approval: DrcApproval, approve: boolean) => invoke<Update>("pcb_drc_approval", {id, approval, approve}),
  pcbDrc: (id: string) => invoke<DrcReport>("pcb_drc", { id }),
  pcbDrcNear: (id: string, items: ItemId[]) => invoke<DrcViolation[]>("pcb_drc_near", { id, items }),
  pcbRepour: (id: string, zone: ItemId | null) => invoke<Update>("pcb_repour", { id, zone }),
  /** Moving footprints with their tracks kept connected, as a `route` command plan. */
  pcbDragFootprints: (id: string, req: { ids: ItemId[]; delta: Point; angle?: number; flipped?: boolean; center?: Point; mode: "disconnect" | "free" | "preserveAngles"; routeMode: RouteMode; corner: CornerStyle }) =>
    invoke<PcbDragPlan>("pcb_drag_footprints", { id, req }),
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
    invoke<PcbDragPlan>("pcb_drag_each", { id, req }),
  pcbDragTracks: (id:string,req:{ids:ItemId[];delta:Point;point:Point;mode:RouteMode;corner:CornerStyle;tolerance:number})=>invoke<PcbDragPlan>("pcb_drag_tracks",{id,req}),
  pcbTrackEditPlan: (id:string,replace:IdItem[],add:Item[],mode:RouteMode)=>invoke<PcbDragPlan>("pcb_track_edit_plan",{id,replace,add,mode}),
  pcbRoute: async (id: string, req: RouteRequest): Promise<RouteResult> => routeItems(await invoke<RouteResponse>("pcb_route", { id, req })),
  pcbRoutePair: async (id: string, req: RouteRequest, negNet: string, gap: number, negStart: Point | null = null): Promise<[RouteResult, RouteResult]> => {
    const [positive, negative] = await invoke<[RouteResponse, RouteResponse]>("pcb_route_pair", { id, req, negNet, gap, negStart });
    return [routeItems(positive), routeItems(negative)];
  },
  pcbTune: (id: string, req: TuneRequest, expected: readonly TrackItem[] | null = null) =>
    invoke<Update>("pcb_tune", { id, req, expected }),
  pcbTunePreview: (id:string,req:TuneRequest)=>invoke<TrackItem[]>("pcb_tune_preview",{id,req}),
  pcbLengthConstraint: (id:string,net:string)=>invoke<LengthConstraint|null>("pcb_length_constraint",{id,net}),
  pcbNetLength: (id: string, net: string) => invoke<number>("pcb_net_length", { id, net }),
  /** A via for `net`; with `from`/`to` the board's matching via type (blind, buried, micro) is used, else a through via. */
  pcbVia: async (id: string, net: string, pos: Point, from: number | null = null, to: number | null = null): Promise<ViaItem> => ({
    ...await invoke<Omit<ViaItem, "kind">>("pcb_via", { id, net, pos, from, to }), kind: "via",
  }),
  pcbPairViaClearance: (id:string,positive:ViaItem,negative:ViaItem,source:number)=>invoke<number>("pcb_pair_via_clearance",{id,positive,negative,source}),
  pcbPairViaPlan: async (id:string,req:{positive:ViaItem;negative:ViaItem;source:number;target:number;direction:Point;width:number;corner:CornerStyle;mode:RouteMode;tracks:TrackItem[];moved:IdItem[]})=>{
    const plan=await invoke<{tracks:Omit<TrackItem,'kind'>[];vias:Omit<ViaItem,'kind'>[]}>("pcb_pair_via_plan",{id,req});
    return {tracks:plan.tracks.map(t=>({...t,kind:'track' as const})),vias:plan.vias.map(v=>({...v,kind:'via' as const}))};
  },
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
  outIpc2581: (id: string, path: string, variant: string | null) => invoke<void>("out_ipc2581", { id, path, variant }),
  outBoardModel: (id: string) => invoke<BoardModel3d>("out_board_model", { id }),
  outStep: (path: string, name: string, meshes: Mesh3d[]) => invoke<void>("out_step", { path, name, meshes }),
  readGerbers: (path: string) => invoke<GerberFile[]>("out_read_gerbers", { path }),
  writeFileBytes: (path: string, bytes: Uint8Array) => invoke<void>("write_file_bytes", { path, bytes: Array.from(bytes) }),
  readFileBytes: (path: string) => invoke<ArrayBuffer>("read_file_bytes", { path }),
  resolveModelPath: (path: string, base: string | null) => invoke<string | null>("resolve_model_path", { path, base }),

  hubPreview: (root:string,path:string) => invoke<SavedPreview>("hub_preview",{root,path}),
  hubBrowse: (root:string,relative:string) => invoke<HubEntry[]>("hub_browse",{root,relative}),
  hubCreateFolder: (root:string,relative:string,name:string) => invoke<string>("hub_create_folder",{root,relative,name}),
  hubCreateProject: (root:string,relative:string,name:string) => invoke<ProjectState>("hub_create_project",{root,relative,name}),
  recoveryCapture: (force:boolean)=>invoke<RecoveryInfo|null>("recovery_capture",{force}),
  recoveryList: ()=>invoke<RecoveryListing>("recovery_list"),
  recoveryRestore: (id:string,dest:string)=>invoke<RecoveryRestored>("recovery_restore",{id,dest}),
  projectCheckpoint: (projectPath:string,label:string) => invoke<RevisionInfo>("project_checkpoint",{projectPath,label}),
  projectCompareRevisions: (projectPath:string,from:string,to:string) => invoke<RevisionChanges>("project_compare_revisions",{projectPath,from,to}),
  projectRevisions: (projectPath:string) => invoke<RevisionInfo[]>("project_revisions",{projectPath}),
  projectRestoreCopy: (projectPath:string,revision:string,dest:string) => invoke<string>("project_restore_copy",{projectPath,revision,dest}),
  importPreview: (request:ImportRequest) => invoke<ImportReview>("import_preview",{request}),
  importDiscard: (token:string) => invoke<void>("import_discard",{token}),
  importApply: (token:string,dest:string) => invoke<{project:ProjectState|null;library:LibInfo|null;files:string[]}>("import_apply",{token,dest}),

  // import
  importLibrary: (source: LibrarySource, path: string) =>
    invoke<{ library: LibInfo; report: ReportLine[] }>("import_library", { source, path }),
  importDesign: (source: DesignSource, path: string, destDir: string) =>
    invoke<{ project: ProjectState; report: ReportLine[]; files: string[] }>("import_design", { source, path, destDir }),
  libraryPaths: () => invoke<{ symbols: string | null; footprints: string | null }>("library_paths"),
  listLibraryFiles: (dir: string) => invoke<string[]>("list_library_files", { dir }),
};

/** Footprint wizard spec: `{ family: "...", ...params }` (see libgen::wizard). */
export interface WizardGoals { toe: number; heel: number; side: number }

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
