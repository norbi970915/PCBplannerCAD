import { highlightNet, navigateTo } from "../../core/actions";
import { selectable } from "../../core/selectFilter";
import { useEditor } from "../../core/store";
import type { IdItem, Item, ItemId, ItemOf, Point } from "../../core/types";
import { connectionPoints } from "../draw";
import { constrain45, deleteVertex, editableRing, insertVertex, isValidRing, moveEdge, moveVertex, nearestOnRing, vertexAt, withRing } from "../polyedit";
import { commitRing, HANDLE_PX, ringHandles } from "./polygon";
import type { PointerInfo, Tool, ToolEnv } from "./types";

const DRAG_PX = 4;
const PICK_PX = 5;

type State =
  | { kind: "idle" }
  | { kind: "box"; sx: number; sy: number; additive: boolean; shown: boolean }
  | { kind: "drag"; sx: number; sy: number; origin: Point; ids: ItemId[]; delta: Point; moved: boolean; attach: Attach | null }
  /** Dragging one outline vertex (also a freshly inserted one). */
  | { kind: "vertex"; id: ItemId; item: Item; ring: Point[]; index: number; ring2: Point[]; sx: number; sy: number; moved: boolean }
  /** Dragging an outline edge parallel to itself. */
  | { kind: "edge"; id: ItemId; item: Item; ring: Point[]; edge: number; origin: Point; ring2: Point[]; sx: number; sy: number; moved: boolean };

/** The single selected polygon outline on a board, if any. */
function editedPolygon(env: ToolEnv): { id: ItemId; item: Item; ring: Point[] } | null {
  const doc = env.doc();
  if ((doc.info.kind !== "pcb" && doc.info.kind !== "footprint") || doc.selection.size !== 1) return null;
  const id = [...doc.selection][0];
  const item = doc.items.get(id);
  const ring = item ? editableRing(item) : null;
  return item && ring ? { id, item, ring } : null;
}

/** Schematic objects that follow dragged parts so connections stay intact. */
interface Attach {
  /** Wires with one end on a moved connection point: that end stretches. */
  stretch: { id: ItemId; wire: ItemOf<"wire">; a: boolean; b: boolean }[];
  /** Junctions, labels, ports and fully attached wires that move along. */
  follow: ItemId[];
}

const pointKey = (p: Point) => `${p.x},${p.y}`;

function attachedTo(env: ToolEnv, ids: ItemId[]): Attach {
  const doc = env.doc();
  const none = { stretch: [], follow: [] };
  if (doc.info.kind !== "schematic") return none;
  const moving = new Set(ids);
  const ctx = env.ctx();
  const points = new Set<string>();
  for (const id of ids) {
    const item = doc.items.get(id);
    if (item) for (const p of connectionPoints(item, ctx)) points.add(pointKey(p));
  }
  const out: Attach = { stretch: [], follow: [] };
  for (const [id, item] of doc.items) {
    if (moving.has(id)) continue;
    if (item.kind === "wire") {
      const a = points.has(pointKey(item.a));
      const b = points.has(pointKey(item.b));
      if (a && b) out.follow.push(id);
      else if (a || b) out.stretch.push({ id, wire: item, a, b });
    } else if (item.kind === "junction" || item.kind === "netLabel" || item.kind === "powerPort" || item.kind === "noConnect") {
      if (points.has(pointKey(item.pos))) out.follow.push(id);
    }
  }
  return out;
}

function stretched(r: Attach["stretch"][number], dx: number, dy: number): ItemOf<"wire"> {
  return {
    kind: "wire",
    a: r.a ? { x: r.wire.a.x + dx, y: r.wire.a.y + dy } : r.wire.a,
    b: r.b ? { x: r.wire.b.x + dx, y: r.wire.b.y + dy } : r.wire.b,
  };
}

/** Select, area-select (→ inside, ← touching) and drag-move. */
export function selectTool(): Tool {
  let s: State = { kind: "idle" };
  let excluded: ReadonlySet<ItemId> = new Set();

  const far = (e: PointerInfo, sx: number, sy: number) => Math.hypot(e.sx - sx, e.sy - sy) > DRAG_PX;

  return {
    id: "select",
    hint: (env) =>
      editedPolygon(env)
        ? "Polygon: drag a corner · drag a mid-point to add a corner · drag an edge to move it · Ctrl+click or Backspace removes a corner · Shift: 45° · E R reshapes"
        : "Click to select · Drag to move · Space rotate · X mirror · Alt+click highlights a net · Drag empty space to area-select",
    reset() {
      s = { kind: "idle" };
      excluded = new Set();
    },
    down(env, e) {
      const doc = env.doc();
      const p = env.raw();
      // Outline handles of the selected pour / keep-out / board outline.
      const poly = editedPolygon(env);
      if (poly && !e.shift && !e.alt) {
        const tol = HANDLE_PX / env.scale();
        const vi = vertexAt(poly.ring, p, tol);
        if (vi >= 0) {
          if (e.ctrl) {
            if (poly.ring.length > 3) void commitRing(env, poly.id, poly.item, deleteVertex(poly.ring, vi));
            return;
          }
          s = { kind: "vertex", ...poly, index: vi, ring2: poly.ring, sx: e.sx, sy: e.sy, moved: false };
          excluded = new Set([poly.id]);
          return;
        }
        const n = poly.ring.length;
        for (let i = 0; i < n; i++) {
          const a = poly.ring[i];
          const b = poly.ring[(i + 1) % n];
          const mid = { x: Math.round((a.x + b.x) / 2), y: Math.round((a.y + b.y) / 2) };
          if (Math.hypot(mid.x - p.x, mid.y - p.y) <= tol * 0.8) {
            const ring = insertVertex(poly.ring, i, mid);
            s = { kind: "vertex", ...poly, ring, index: i + 1, ring2: ring, sx: e.sx, sy: e.sy, moved: false };
            excluded = new Set([poly.id]);
            return;
          }
        }
        const hit = nearestOnRing(poly.ring, p);
        if (hit && hit.dist <= tol * 0.8) {
          if (e.ctrl) {
            const ring = insertVertex(poly.ring, hit.edge, hit.point);
            s = { kind: "vertex", ...poly, ring, index: hit.edge + 1, ring2: ring, sx: e.sx, sy: e.sy, moved: false };
          } else {
            s = { kind: "edge", ...poly, edge: hit.edge, origin: env.cursor(), ring2: poly.ring, sx: e.sx, sy: e.sy, moved: false };
          }
          excluded = new Set([poly.id]);
          return;
        }
      }
      const kind = doc.info.kind;
      const hit = env.scene.pick(p, PICK_PX / env.scale(), (i) => selectable(kind, i));
      if (hit === null) {
        s = { kind: "box", sx: e.sx, sy: e.sy, additive: e.shift, shown: false };
        return;
      }
      if (e.alt && doc.info.kind === "schematic") {
        void highlightNet(env.docId, hit);
        return;
      }
      if (e.shift || e.ctrl) {
        const next = new Set(doc.selection);
        if (next.has(hit)) next.delete(hit);
        else next.add(hit);
        env.select(next);
        return;
      }
      if (!doc.selection.has(hit)) env.select([hit]);
      s = { kind: "drag", sx: e.sx, sy: e.sy, origin: env.cursor(), ids: [...env.doc().selection], delta: { x: 0, y: 0 }, moved: false, attach: null };
    },
    move(env, e) {
      if (s.kind === "vertex") {
        s.moved ||= far(e, s.sx, s.sy);
        const prev = s.ring[(s.index + s.ring.length - 1) % s.ring.length];
        const to = e.shift ? constrain45(prev, env.cursor()) : env.cursor();
        s.ring2 = moveVertex(s.ring, s.index, to);
        return;
      }
      if (s.kind === "edge") {
        s.moved ||= far(e, s.sx, s.sy);
        const c = env.cursor();
        s.ring2 = moveEdge(s.ring, s.edge, { x: c.x - s.origin.x, y: c.y - s.origin.y });
        return;
      }
      if (s.kind === "box") {
        s.shown ||= far(e, s.sx, s.sy);
        if (s.shown) {
          env.setBox({ x: Math.min(e.sx, s.sx), y: Math.min(e.sy, s.sy), w: Math.abs(e.sx - s.sx), h: Math.abs(e.sy - s.sy), touching: e.sx < s.sx });
        }
      } else if (s.kind === "drag") {
        const wasMoved = s.moved;
        s.moved ||= far(e, s.sx, s.sy);
        const c = env.cursor();
        s.delta = { x: c.x - s.origin.x, y: c.y - s.origin.y };
        if (s.moved && (!wasMoved || !s.attach)) {
          s.attach = attachedTo(env, s.ids);
          excluded = new Set([...s.ids, ...s.attach.follow, ...s.attach.stretch.map((r) => r.id)]);
          env.redraw(true);
        }
      }
    },
    up(env, e) {
      const done = s;
      s = { kind: "idle" };
      if (done.kind === "vertex" || done.kind === "edge") {
        excluded = new Set();
        const inserted = done.kind === "vertex" && done.ring.length !== editableRing(done.item)!.length;
        if ((done.moved || inserted) && isValidRing(done.ring2)) void commitRing(env, done.id, done.item, done.ring2).then(() => env.redraw(true));
        else {
          if (done.moved) useEditor.getState().log("warning", "That edit would make the outline cross itself — not applied.");
          env.redraw(true);
        }
        return;
      }
      if (done.kind === "box") {
        env.setBox(null);
        if (!done.shown) {
          if (!done.additive) {
            env.select([]);
            useEditor.getState().patchDoc(env.docId, { marked: new Set() });
          }
          return;
        }
        const a = env.toWorld(done.sx, done.sy);
        const b = env.toWorld(e.sx, e.sy);
        const kind = env.doc().info.kind;
        const ids = env.scene.inRect(
          { minX: Math.min(a.x, b.x), minY: Math.min(a.y, b.y), maxX: Math.max(a.x, b.x), maxY: Math.max(a.y, b.y) },
          e.sx < done.sx,
          (i) => selectable(kind, i),
        );
        env.select(done.additive ? [...env.doc().selection, ...ids] : ids);
      } else if (done.kind === "drag") {
        const moved = done.moved && (done.delta.x || done.delta.y);
        const finish = () => {
          excluded = new Set();
          env.redraw(true);
        };
        if (!moved) return finish();
        const { x: dx, y: dy } = done.delta;
        const attach = done.attach ?? { stretch: [], follow: [] };
        if (!attach.stretch.length && !attach.follow.length) {
          // Moved pours keep their old copper until refilled at the new place.
          const pours = done.ids.filter((id) => {
            const it = env.doc().items.get(id);
            return it?.kind === "zone" && !it.keepout;
          });
          void env.exec({ op: "move", ids: done.ids, dx, dy }).then(async (ok) => {
            finish();
            if (!ok || !pours.length) return;
            const { repour } = await import("../../core/pcbActions");
            for (const id of pours) await repour(env.docId, id, true);
          });
          return;
        }
        // One undo step: moved items, followers and stretched wires together.
        const items = env.doc().items;
        const replaced: IdItem[] = [...done.ids, ...attach.follow].flatMap((id) => {
          const it = items.get(id);
          return it ? [{ id, item: translate(it, dx, dy) }] : [];
        });
        for (const r of attach.stretch) {
          const w = stretched(r, dx, dy);
          if (w.a.x !== w.b.x || w.a.y !== w.b.y) replaced.push({ id: r.id, item: w });
        }
        void env.exec({ op: "replaceMany", items: replaced }).then(finish);
      }
    },
    dblclick(env) {
      const hit = env.scene.pick(env.raw(), PICK_PX / env.scale());
      const item = hit !== null ? env.doc().items.get(hit) : undefined;
      if (item?.kind === "sheet") void navigateTo({ sheet: item.file });
    },
    key(env, key) {
      const doc = env.doc();
      const poly = s.kind === "idle" ? editedPolygon(env) : null;
      if (poly && key === "Backspace") {
        const vi = vertexAt(poly.ring, env.raw(), HANDLE_PX / env.scale());
        if (vi >= 0 && poly.ring.length > 3) {
          void commitRing(env, poly.id, poly.item, deleteVertex(poly.ring, vi));
          return true;
        }
      }
      if (s.kind === "drag" && s.moved && (key === " " || key === "x" || key === "X")) {
        // Rotate/mirror the dragged items in place around the grab point.
        const center = { x: s.origin.x, y: s.origin.y };
        const ids = s.ids;
        if (key === " ") void env.exec({ op: "rotate", ids, center, angle: 90 });
        else void env.exec({ op: "mirror", ids, center });
        // Pins moved: work out attached wires again on the next pointer move.
        s.attach = null;
        return true;
      }
      if (!doc.selection.size) return false;
      if (key === " ") {
        void env.exec({ op: "rotate", ids: [...doc.selection], center: env.cursor(), angle: 90 });
        return true;
      }
      if (key === "x" || key === "X" || ((key === "l" || key === "L") && doc.info.kind === "pcb")) {
        void env.exec({ op: "mirror", ids: [...doc.selection], center: env.cursor() });
        return true;
      }
      return false;
    },
    cancel(env) {
      if (s.kind === "idle") return false;
      if (s.kind === "box") env.setBox(null);
      s = { kind: "idle" };
      excluded = new Set();
      env.redraw(true);
      return true;
    },
    preview(env) {
      if (s.kind === "vertex" || s.kind === "edge") return [withRing(s.item, s.ring2)];
      if (s.kind !== "drag" || !s.moved) return [];
      const items = env.doc().items;
      const { x: dx, y: dy } = s.delta;
      const attach = s.attach ?? { stretch: [], follow: [] };
      const moved = [...s.ids, ...attach.follow]
        .map((id) => items.get(id))
        .filter((i): i is Item => !!i)
        .map((i) => translate(i, dx, dy));
      return [...moved, ...attach.stretch.map((r) => stretched(r, dx, dy))];
    },
    excluded: () => excluded,
    // Corner and edge drags of a polygon snap to pads, track ends and corners.
    snaps: () => s.kind === "vertex" || s.kind === "edge",
    overlay(env) {
      const px = 1 / env.scale();
      if (s.kind === "vertex" || s.kind === "edge") return ringHandles(s.ring2, px);
      if (s.kind === "drag" && s.moved) return [];
      const poly = editedPolygon(env);
      return poly ? ringHandles(poly.ring, px) : [];
    },
  };
}

/** Client-side translate for previews (mirrors Item::translated in Rust). */
export function translate(item: Item, dx: number, dy: number): Item {
  const t = (p: Point) => ({ x: p.x + dx, y: p.y + dy });
  switch (item.kind) {
    case "line":
    case "wire":
    case "bus":
    case "rect":
    case "track":
      return { ...item, a: t(item.a), b: t(item.b) };
    case "arc":
    case "circle":
    case "arcTrack":
      return { ...item, center: t(item.center) };
    case "polyline":
      return { ...item, points: item.points.map(t) };
    case "zone":
      return { ...item, outline: item.outline.map(t), fill: item.fill.map((f) => ({ outer: f.outer.map(t), holes: f.holes.map((h) => h.map(t)) })) };
    case "board":
      return item;
    default:
      return { ...item, pos: t(item.pos) } as Item;
  }
}
