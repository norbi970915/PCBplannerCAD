// Mirrors the serde types in crates/core and app/src-tauri.
// All lengths are integer nanometers; world Y points up; angles are degrees CCW.

export type Coord = number;
export type ItemId = number;
export type LayerId = number;

export interface Point {
  x: Coord;
  y: Coord;
}

export type FillStyle = "none" | "solid" | "background";
export type HAlign = "left" | "center" | "right";
export type VAlign = "bottom" | "middle" | "top";

export interface LineItem {
  kind: "line";
  a: Point;
  b: Point;
  width: Coord;
  layer: LayerId;
}

export interface ArcItem {
  kind: "arc";
  center: Point;
  radius: Coord;
  startAngle: number;
  sweep: number;
  width: Coord;
  layer: LayerId;
}

export interface CircleItem {
  kind: "circle";
  center: Point;
  radius: Coord;
  width: Coord;
  layer: LayerId;
  fill: FillStyle;
}

export interface RectItem {
  kind: "rect";
  a: Point;
  b: Point;
  width: Coord;
  layer: LayerId;
  fill: FillStyle;
}

export interface PolylineItem {
  kind: "polyline";
  points: Point[];
  closed: boolean;
  width: Coord;
  layer: LayerId;
  fill: FillStyle;
}

export interface TextItem {
  kind: "text";
  pos: Point;
  text: string;
  height: Coord;
  rotation: number;
  hAlign: HAlign;
  vAlign: VAlign;
  layer: LayerId;
  mirrored: boolean;
}

export type PinType =
  | "input"
  | "output"
  | "bidirectional"
  | "triState"
  | "passive"
  | "powerIn"
  | "powerOut"
  | "openCollector"
  | "openEmitter"
  | "noConnect"
  | "unspecified";

export type PinShape = "line" | "inverted" | "clock" | "invertedClock";

export interface PinItem {
  kind: "pin";
  pos: Point;
  length: Coord;
  rotation: number;
  name: string;
  number: string;
  electrical: PinType;
  shape: PinShape;
  hidden: boolean;
}

export type PadShape = "circle" | "rect" | "roundRect" | "oval";
export type PadKind = "smd" | "tht" | "npth";

export interface Drill {
  width: Coord;
  height: Coord;
}

export interface PadItem {
  kind: "pad";
  number: string;
  pos: Point;
  rotation: number;
  shape: PadShape;
  width: Coord;
  height: Coord;
  cornerRatio: number;
  /** `kind` in Rust; renamed on the wire because `kind` is the item tag. */
  padKind: PadKind;
  layer: LayerId;
  drill: Drill | null;
  copperShapes?: Record<number,{shape:PadShape; width:number; height:number; cornerRatio:number}>;
}

export interface WireItem {
  kind: "wire";
  a: Point;
  b: Point;
}

export interface BusItem {
  kind: "bus";
  a: Point;
  b: Point;
}

export interface BusEntryItem {
  kind: "busEntry";
  pos: Point;
  size: Point;
}

export interface JunctionItem {
  kind: "junction";
  pos: Point;
}

export interface NoConnectItem {
  kind: "noConnect";
  pos: Point;
}

export type LabelScope = "local" | "global" | "hierarchical";
export type PortShape = "input" | "output" | "bidirectional" | "triState" | "passive";

export interface NetLabelItem {
  kind: "netLabel";
  pos: Point;
  text: string;
  rotation: number;
  scope: LabelScope;
  shape: PortShape;
}

export type PowerStyle = "bar" | "arrow" | "circle" | "wave" | "ground" | "signalGround" | "earth";

export interface PowerPortItem {
  kind: "powerPort";
  pos: Point;
  net: string;
  rotation: number;
  style: PowerStyle;
}

export interface Field {
  name: string;
  value: string;
  offset: Point;
  rotation: number;
  height: Coord;
  hAlign: HAlign;
  vAlign: VAlign;
  visible: boolean;
}

export interface PartItem {
  kind: "part";
  pos: Point;
  rotation: number;
  mirror: boolean;
  libRef: string;
  unit: number;
  fields: Field[];
}

export type SheetSide = "left" | "right" | "top" | "bottom";

export interface SheetEntry {
  name: string;
  side: SheetSide;
  offset: Coord;
  shape: PortShape;
}

export interface SheetItem {
  kind: "sheet";
  pos: Point;
  size: Point;
  name: string;
  file: string;
  entries: SheetEntry[];
}

// ---------- Board items ----------

export type Side = "top" | "bottom";

export interface PlacedFootprintItem {
  boardOnly?: boolean;
  kind: "footprint";
  designator: string;
  value: string;
  footprint: string;
  pos: Point;
  rotation: number;
  side: Side;
  locked: boolean;
  padNets: Record<string, string>;
  component: string;
  parameters: Record<string, string>;
}

export interface TrackItem {
  locked?: boolean;
  kind: "track";
  a: Point;
  b: Point;
  width: Coord;
  layer: LayerId;
  net: string;
}

export interface ArcTrackItem {
  locked?: boolean;
  kind: "arcTrack";
  center: Point;
  radius: Coord;
  startAngle: number;
  sweep: number;
  width: Coord;
  layer: LayerId;
  net: string;
}

export interface ViaItem {
  locked?: boolean;
  kind: "via";
  pos: Point;
  diameter: Coord;
  drill: Coord;
  fromLayer: LayerId;
  toLayer: LayerId;
  net: string;
}

export type ZoneConnect = "thermal" | "solid" | "none";

export interface ZonePolygon {
  outer: Point[];
  holes: Point[][];
}

export interface ZoneItem {
  locked?: boolean;
  kind: "zone";
  outline: Point[];
  layer: LayerId;
  net: string;
  name: string;
  priority: number;
  clearance: Coord;
  minWidth: Coord;
  connect: ZoneConnect;
  thermalGap: Coord;
  spokeWidth: Coord;
  keepout: boolean;
  removeIslands: boolean;
  fill: ZonePolygon[];
  /** Teardrop copper (Tools ▸ Teardrops), not a user pour. */
  teardrop?: boolean;
}

export type StackKind = "copper" | "dielectric" | "solderMask" | "silkscreen" | "paste";

export interface StackLayer {
  name: string;
  kind: StackKind;
  layer: LayerId | null;
  thickness: Coord;
  material: string;
  epsilonR: number;
}

export type RuleKind =
  | { type: "clearance"; first: string; second: string; min: Coord }
  | { type: "width"; scope: string; min: Coord; preferred: Coord; max: Coord }
  | { type: "viaStyle"; scope: string; diameter: Coord; drill: Coord; minAnnularRing: Coord }
  | { type: "holeSize"; scope: string; min: Coord; max: Coord }
  | { type: "boardOutlineClearance"; min: Coord }
  | { type: "componentClearance"; min: Coord }
  | { type: "solderMaskExpansion"; scope: string; expansion: Coord }
  | { type: "pasteMaskExpansion"; scope: string; expansion: Coord }
  | { type: "zoneConnect"; scope: string; style: ZoneConnect; gap: Coord; spokeWidth: Coord }
  | { type: "diffPair"; scope: string; width: Coord; gap: Coord }
  | { type: "length"; scope: string; min: Coord; max: Coord }
  | { type: "silkClearance"; min: Coord };

export type Rule = { name: string; enabled: boolean } & RuleKind;

export interface NetClass {
  name: string;
  nets: string[];
  trackWidth: Coord;
  clearance: Coord;
  viaDiameter: Coord;
  viaDrill: Coord;
  diffPairWidth: Coord;
  diffPairGap: Coord;
}

export interface DiffPair {
  name: string;
  positive: string;
  negative: string;
}

/** A via the board may use; routing picks it for matching layer changes and DRC accepts it. */
export interface ViaType {
  name: string;
  fromLayer: LayerId;
  toLayer: LayerId;
  diameter: Coord;
  drill: Coord;
}

export interface BoardSettings {
  autoTeardrops?: TeardropOptions | null;
  stackup: { layers: StackLayer[] };
  viaTypes?: ViaType[];
  rules: Rule[];
  netClasses: NetClass[];
  diffPairs: DiffPair[];
  origin: Point;
  maskColor: string;
  silkColor: string;
}

/** A measured distance (see crates/core/src/pcb/dimension.rs). */
export interface DimensionItem {
  kind: "dimension";
  a: Point;
  b: Point;
  /** Distance of the dimension line from `a` along the left normal of the measuring direction. */
  offset: Coord;
  /** Measure along `angle` (0 = horizontal, 90 = vertical) instead of along a→b. */
  orthogonal: boolean;
  angle: number;
  layer: LayerId;
  width: Coord;
  textHeight: Coord;
  units: "mm" | "mil" | "inch";
  precision: number;
}

export interface BoardItem extends BoardSettings {
  kind: "board";
}

export type Item =
  | LineItem
  | ArcItem
  | CircleItem
  | RectItem
  | PolylineItem
  | TextItem
  | PinItem
  | PadItem
  | WireItem
  | BusItem
  | BusEntryItem
  | JunctionItem
  | NoConnectItem
  | NetLabelItem
  | PowerPortItem
  | PartItem
  | SheetItem
  | PlacedFootprintItem
  | TrackItem
  | ArcTrackItem
  | ViaItem
  | ZoneItem
  | DimensionItem
  | BoardItem;

export type ItemKind = Item["kind"];
export type ItemOf<K extends ItemKind> = Extract<Item, { kind: K }>;

export interface IdItem {
  id: ItemId;
  item: Item;
}

/** Connected move plan; failure means the entire plan is rejected. */
export interface PcbDragPlan {
  replace: IdItem[];
  delete: ItemId[];
  add: Item[];
  rerouted: number;
  blocked: number;
  failure?: "locked" | "unroutable";
  message?: string;
}

// ---------- Library ----------

export interface SymbolUnit {
  name: string;
  items: Item[];
}

export interface LibSymbol {
  name: string;
  description: string;
  prefix: string;
  units: SymbolUnit[];
  showPinNumbers: boolean;
  showPinNames: boolean;
  pinNameOffset: Coord;
  isPower: boolean;
  fields: Field[];
}

export interface Model3d {
  path: string;
  offset: [number, number, number];
  rotation: [number, number, number];
  scale: [number, number, number];
}

export interface Footprint {
  name: string;
  description: string;
  items: Item[];
  model: Model3d | null;
}

export interface PinMapping {
  unit: number;
  pin: string;
  pads: string[];
}

export interface Parameter {
  name: string;
  value: string;
}

export interface Component {
  name: string;
  description: string;
  symbol: string;
  footprints: string[];
  pinMap: PinMapping[];
  parameters: Parameter[];
}

export interface Library {
  name: string;
  symbols: LibSymbol[];
  footprints: Footprint[];
  components: Component[];
}

export interface CachedPart {
  library: string;
  component: Component;
  symbol: LibSymbol;
}

// ---------- Workspace ----------

export type DocKind = "pcb" | "schematic" | "symbol" | "footprint";

export interface DocInfo {
  id: string;
  kind: DocKind;
  title: string;
  path: string | null;
  dirty: boolean;
  canUndo: boolean;
  canRedo: boolean;
  library: string | null;
  entry: string | null;
  unit: number | null;
  inProject: boolean;
}

export interface Delta {
  upserted: IdItem[];
  removed: ItemId[];
}

export interface Snapshot {
  info: DocInfo;
  items: IdItem[];
  cache: Record<string, CachedPart>;
  /** PCB documents: footprint geometry for placed footprints. */
  footprints: Record<string, Footprint>;
}

// ---------- Board analysis ----------

export interface AirWire {
  net: string;
  a: Point;
  b: Point;
}

export interface RoutingStats {
  nets: number;
  connections: number;
  unrouted: number;
}

export interface DrcViolation {
  severity: "error" | "warning";
  code: string;
  rule: string;
  message: string;
  items: ItemId[];
  pos: Point;
  layer: LayerId | null;
}

export interface DrcReport {
  violations: DrcViolation[];
  stats: RoutingStats;
  ruleErrors: string[];
}

export type RouteMode = "ignore" | "walkaround" | "shove";
export type CornerStyle = "diagonal45" | "arc45" | "orthogonal90" | "any";

export interface RouteRequest {
  net: string;
  layer: LayerId;
  start: Point;
  end: Point;
  width: Coord;
  mode: RouteMode;
  corner: CornerStyle;
  ignore: ItemId[];
  postureFlip: boolean;
}

export interface RouteResult {
  tracks: TrackItem[];
  reached: boolean;
  moved: IdItem[];
  blockedAt: Point | null;
  message: string;
}

export type EcoChange =
  | { kind: "addFootprint"; designator: string; value: string; footprintKey: string; component: string; padNets: Record<string, string> }
  | { kind: "removeFootprint"; item: ItemId; designator: string }
  | { kind: "changeFootprint"; item: ItemId; designator: string; from: string; to: string }
  | { kind: "updateValue"; item: ItemId; designator: string; from: string; to: string }
  | { kind: "updateNets"; item: ItemId; designator: string; changes: [string, string, string][] }
  | { kind: "updateParameters"; item: ItemId; designator: string }
  | { kind: "renameNet"; from: string; to: string }
  | { kind: "warning"; message: string };

export interface Mesh3d {
  name: string;
  positions: number[];
  indices: number[];
  color: [number, number, number];
}

export interface ComponentPlacement3d {
  designator: string;
  footprint: string;
  model: Model3d | null;
  transform: number[];
  fallbackBox: [number, number, number, number, number, number];
}

export interface BoardModel3d {
  board: Mesh3d;
  thicknessMm: number;
  outlineFound: boolean;
  components: ComponentPlacement3d[];
}

export interface Update {
  delta: Delta;
  info: DocInfo;
  touchedLibrary: string | null;
}

export type Command =
  | { op: "editPours"; items: IdItem[]; refill: ItemId[] | null }
  | { op: "add"; items: Item[] }
  | { op: "delete"; ids: ItemId[] }
  | { op: "move"; ids: ItemId[]; dx: Coord; dy: Coord }
  | { op: "rotate"; ids: ItemId[]; center: Point; angle: number }
  | { op: "mirror"; ids: ItemId[]; center: Point }
  | { op: "replace"; id: ItemId; item: Item }
  | { op: "replaceMany"; items: IdItem[] }
  | { op: "change"; delete: ItemId[]; add: Item[] }
  | { op: "route"; delete: ItemId[]; add: Item[]; replace?: IdItem[] }
  | { op: "editPolygon"; id: ItemId; original: Point[]; outlines: Point[][] };

export interface LibInfo {
  id: string;
  name: string;
  path: string | null;
  dirty: boolean;
  readonly: boolean;
  symbols: number;
  footprints: number;
  components: number;
  inProject: boolean;
}

export interface ComponentEntry {
  library: string;
  libraryName: string;
  name: string;
  description: string;
  value: string;
  symbol: string;
  footprints: string[];
  units: number;
  pins: number;
}

export interface ProjectSheet {
  file: string;
  docId: string | null;
}

export interface ProjectInfo {
  path: string;
  name: string;
  sheets: ProjectSheet[];
  libraries: string[];
  pcb: ProjectSheet | null;
  variants: { name: string; dnp: string[] }[];
}

export interface ProjectState {
  project: ProjectInfo | null;
  messages: string[];
}

// ---------- Compilation ----------

export interface NetNode {
  designator: string;
  pinNumber: string;
  pinName: string;
  pinType: PinType;
  pads: string[];
  sheet: string;
  part: ItemId;
}

export interface Net {
  name: string;
  nodes: NetNode[];
  items: [string, ItemId][];
}

export interface NetlistPart {
  designator: string;
  value: string;
  footprint: string;
  libRef: string;
  component: string;
  library: string;
  units: [string, ItemId, number][];
  parameters: [string, string][];
}

export interface Violation {
  severity: "error" | "warning";
  code: string;
  message: string;
  sheet: string;
  items: ItemId[];
  pos: Point | null;
}

export interface Compiled {
  nets: Net[];
  parts: NetlistPart[];
  violations: Violation[];
}

export interface ReportLine {
  warning: boolean;
  text: string;
}

/** Tools ▸ Cleanup Tracks & Vias: what to remove or merge. */
export interface CleanupOptions {
  zeroLength: boolean;
  duplicates: boolean;
  dangling: boolean;
  unconnectedVias: boolean;
  mergeCollinear: boolean;
}

export interface CleanupResult {
  delete: ItemId[];
  replace: IdItem[];
  zeroLength: number;
  duplicates: number;
  dangling: number;
  vias: number;
  merged: number;
}

/** Tools ▸ Teardrops. */
export interface TeardropOptions {
  vias: boolean;
  pthPads: boolean;
  smdPads: boolean;
  lengthRatio: number;
  widthRatio: number;
  maxLength: Coord;
  maxWidth: Coord;
  curved: boolean;
  maxTrackRatio: number;
}
