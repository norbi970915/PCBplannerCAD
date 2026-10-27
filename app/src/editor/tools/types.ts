import type { DocState, Placement } from "../../core/store";
import type { Command, Item, ItemId, Point } from "../../core/types";
import type { DrawContext } from "../draw";
import type { DrawFill, DrawLine } from "../renderer";
import type { Scene } from "../scene";

export interface PointerInfo {
  sx: number;
  sy: number;
  shift: boolean;
  ctrl: boolean;
  alt: boolean;
}

export interface ScreenRect {
  x: number;
  y: number;
  w: number;
  h: number;
  touching: boolean;
}

/** What a tool can see and do; provided by the canvas view. */
export interface ToolEnv {
  docId: string;
  doc(): DocState;
  /** Snapped cursor (grid, or a nearby connection point in schematics). */
  cursor(): Point;
  /** Unsnapped world position of the pointer. */
  raw(): Point;
  /** Screen pixels per nanometer. */
  scale(): number;
  scene: Scene;
  ctx(): DrawContext;
  exec(cmd: Command): Promise<boolean>;
  placement(): Placement;
  setPlacement(p: Partial<Placement>): void;
  select(ids: Iterable<ItemId>): void;
  setTool(id: string): void;
  setBox(b: ScreenRect | null): void;
  toWorld(sx: number, sy: number): Point;
  /** Re-render overlay (and scene when `scene` is true). */
  redraw(scene?: boolean): void;
  /** Graphic layer for new items. */
  layer(): number;
}

export interface Tool {
  id: string;
  hint(env: ToolEnv): string;
  down?(env: ToolEnv, e: PointerInfo): void;
  move?(env: ToolEnv, e: PointerInfo): void;
  up?(env: ToolEnv, e: PointerInfo): void;
  dblclick?(env: ToolEnv, e: PointerInfo): void;
  /** Keyboard while the canvas is active. Return true when handled. */
  key?(env: ToolEnv, key: string, e: KeyboardEvent): boolean;
  /** Esc / right-click. Return true if an operation in progress was cancelled. */
  cancel(env: ToolEnv, source?: "key" | "mouse"): boolean;
  preview?(env: ToolEnv): Item[];
  /** Screen-space marks drawn on top (handles, rubber bands); widths in world units. */
  overlay?(env: ToolEnv): DrawLine[];
  /** Translucent editing regions, independent of the board's fill visibility. */
  overlayFills?(env: ToolEnv): DrawFill[];
  /** Items hidden from the scene while the tool shows them elsewhere (dragging). */
  excluded?(): ReadonlySet<ItemId>;
  /** Snap the cursor to objects (pads, track ends, corners). Default: every tool but select. */
  snaps?(): boolean;
  /** Disable placement-grid snapping while retaining integer-nanometer coordinates. */
  gridSnap?(): boolean;
  /** Called when the tool becomes active / inactive. */
  reset?(): void;
  /** Middle (1) or right (2) press during an operation, e.g. flip or rotate a dragged part. Return true when handled; otherwise the button pans. */
  auxPress?(env: ToolEnv, button: 1 | 2, shift: boolean): boolean;
  /** Attach `ids` to the cursor as a floating move (Pack & Move, new parts from Update PCB). */
  pickUp?(env: ToolEnv, ids: ItemId[]): void;
}
