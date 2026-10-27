import earcut from "earcut";
import type { Item, ItemId, Point } from "../../core/types";
import { layerVisible } from "../../core/view";
import { constrain45, editableRing, nearestOnRing, pointInRing, reshape, type OutlineHit, type Reshape } from "../polyedit";
import type { DrawFill, DrawLine, Rgba } from "../renderer";
import { ringHandles, ringLines } from "./polygon";
import type { Tool, ToolEnv } from "./types";

const SNAP_PX = 10;
const HANDLE: Rgba = [0.3, 0.62, 1, 1], PATH: Rgba = [1, 0.82, 0.25, 1];
const DROP: Rgba = [1, 0.3, 0.3, 1], KEEP: Rgba = [0.3, 0.9, 0.55, 1];
type Mode = "cut" | "slice" | "reshape";
type Path = { kind: "path"; id: ItemId; ring: Point[]; start: OutlineHit; pts: Point[]; choice: 0 | 1 };
type Choice = { kind: "choose"; path: Path; shape: Reshape; boundary: Point[]; remove: 0 | 1 };
type State = { kind: "idle" } | Path | Choice | { kind: "busy" };

function pickOutline(env: ToolEnv) {
  const doc = env.doc(), tol = SNAP_PX/env.scale();
  let best: { id: ItemId; item: Item; hit: OutlineHit; ring: Point[]; score: number } | null = null;
  for (const [id, item] of doc.items) {
    const ring = editableRing(item);
    if (!ring || !("layer" in item) || !layerVisible(doc.view, item.layer)) continue;
    if (item.layer !== doc.activeLayer && (!doc.selection.has(id) || doc.layerMode !== "all")) continue;
    const hit = nearestOnRing(ring, env.raw());
    if (!hit || hit.dist > tol) continue;
    const score = hit.dist - (doc.selection.has(id) ? tol : 0);
    if (!best || score < best.score) best = { id, item, ring, hit, score };
  }
  return best;
}

/** Repeatable boundary → corners → boundary cuts, slices and reshapes. */
export function reshapeTool(mode: Mode = "reshape"): Tool {
  let s: State = { kind: "idle" }, shift = false, error = "", generation = 0;
  let cached: { key: string; shape: Reshape; boundary: Point[] } | null = null;
  let fillCache: { shape: Reshape; triangles: [number[], number[]] } | null = null;
  const title = mode === "cut" ? "Cut" : mode === "slice" ? "Slice" : "Reshape";
  const target = (env: ToolEnv): { point: Point; end: OutlineHit | null } => {
    // The document grid is for placement/routing, not freehand polygon cuts.
    // Keep only the integer-nanometer precision required by the file format.
    const free = { x: Math.round(env.raw().x), y: Math.round(env.raw().y) };
    if (s.kind !== "path") return { point: free, end: null };
    const tol = SNAP_PX/env.scale(), raw = env.raw();
    let hit = nearestOnRing(s.ring, raw);
    if (hit && hit.dist <= tol) {
      const vertex = s.ring.find(v => Math.hypot(v.x-raw.x, v.y-raw.y) <= tol);
      if (vertex) hit = nearestOnRing(s.ring, vertex)!;
      if (Math.hypot(hit.point.x-s.start.point.x, hit.point.y-s.start.point.y) > tol) return { point: hit.point, end: hit };
    }
    return { point: shift ? constrain45(s.pts[s.pts.length-1], free) : free, end: null };
  };
  const result = (env: ToolEnv) => {
    if (s.kind === "choose") return { shape: s.shape, boundary: s.boundary };
    if (s.kind !== "path") return null;
    const t = target(env);
    if (!t.end) return null;
    const key = `${s.pts.length}:${t.end.edge}:${t.end.t}:${t.point.x}:${t.point.y}`;
    if (cached?.key !== key) {
      const boundary = [...s.pts, t.point];
      cached = { key, boundary, shape: reshape(s.ring, s.start, t.end, boundary) };
    }
    return cached;
  };
  const commit = async (env: ToolEnv, path: Path, outlines: Point[][]) => {
    const previous = s, token = generation;
    s = { kind: "busy" }; error = ""; env.redraw();
    let ok = false;
    try { ok = await env.exec({ op: "editPolygon", id: path.id, original: path.ring, outlines }); }
    catch { /* The command adapter reports errors in Messages. */ }
    if (token !== generation) return;
    s = ok ? { kind: "idle" } : previous;
    if (!ok) error = "Could not apply; check Messages or Esc to cancel. ";
    cached = null; fillCache = null; env.redraw(true);
  };
  const finish = (env: ToolEnv) => {
    if (s.kind !== "path") return;
    const r = result(env);
    if (!r) { error = "Finish on the same polygon boundary. "; return; }
    if (mode !== "reshape" && !r.shape.partition) {
      error = "Cut must stay inside without crossing its boundary or itself. "; return;
    }
    if (mode === "cut") {
      s = { kind: "choose", path: s, shape: r.shape, boundary: r.boundary, remove: 1 }; error = "";
    } else if (mode === "slice") void commit(env, s, r.shape.options);
    else if (r.shape.valid[s.choice]) void commit(env, s, [r.shape.options[s.choice]]);
    else error = "Outline crosses or overlaps itself; move the end or undo a corner. ";
  };
  const keepSide = (i: number) => mode === "slice" || (s.kind === "choose" ? i !== s.remove : s.kind === "path" && i === s.choice);
  return {
    id: mode === "reshape" ? "reshape" : mode === "cut" ? "polygonCut" : "polygonSlice",
    snaps: () => false,
    gridSnap: () => false,
    hint: () => {
      if (s.kind === "busy") return `${title}: applying and refilling copper…`;
      if (s.kind === "idle") return `${title}: click a polygon boundary · free angle, no grid · finish on the boundary · Esc exits`;
      if (s.kind === "choose") return `${error}Cut: click the RED side to remove it · Space swaps sides · Backspace edits the cut · Esc cancels`;
      return `${error}${title}: click corners, then the boundary${mode === "reshape" ? ` · Space: keep ${s.choice === 0 ? "larger" : "smaller"} side` : ""} · Shift: 45° · Backspace: undo corner · Esc cancels`;
    },
    reset() { generation++; s = { kind: "idle" }; error = ""; cached = null; fillCache = null; },
    move(env, e) {
      shift = e.shift;
      if (s.kind === "choose") {
        const a = pointInRing(s.shape.options[0], env.raw()), b = pointInRing(s.shape.options[1], env.raw());
        if (a !== b) s.remove = a ? 0 : 1;
      }
    },
    down(env, e) {
      shift = e.shift;
      if (s.kind === "busy") return;
      if (s.kind === "choose") {
        const a = pointInRing(s.shape.options[0], env.raw()), b = pointInRing(s.shape.options[1], env.raw());
        if (a === b) { error = "Click inside one of the two sides. "; return; }
        s.remove = a ? 0 : 1;
        void commit(env, s.path, [s.shape.options[1-s.remove]]); return;
      }
      if (s.kind === "idle") {
        const hit = pickOutline(env);
        if (!hit) return;
        const vertex = hit.ring.find(v => Math.hypot(v.x-env.raw().x, v.y-env.raw().y) <= SNAP_PX/env.scale());
        const start = vertex ? nearestOnRing(hit.ring, vertex)! : hit.hit;
        s = { kind: "path", id: hit.id, ring: hit.ring, start, pts: [start.point], choice: 0 };
        env.select([hit.id]); error = ""; cached = null; return;
      }
      const t = target(env);
      if (t.end) { finish(env); return; }
      const last = s.pts[s.pts.length-1];
      if (t.point.x !== last.x || t.point.y !== last.y) { s.pts.push(t.point); cached = null; error = ""; }
    },
    key(env, key, e) {
      if (s.kind === "busy") return key === " " || key === "Enter" || key === "Backspace";
      if (key === "Shift") { shift = e.shiftKey; return true; }
      if (s.kind === "idle") return false;
      if (key === " ") {
        if (s.kind === "choose") s.remove = (1-s.remove) as 0 | 1;
        else if (mode === "reshape") s.choice = (1-s.choice) as 0 | 1;
        return true;
      }
      if (key === "Backspace") {
        if (s.kind === "choose") s = s.path;
        else if (s.pts.length > 1) s.pts.pop(); else s = { kind: "idle" };
        cached = null; error = ""; return true;
      }
      if (key === "Enter") {
        if (s.kind === "choose") void commit(env, s.path, [s.shape.options[1-s.remove]]); else finish(env);
        return true;
      }
      return false;
    },
    cancel() {
      if (s.kind === "busy") return true;
      if (s.kind === "idle") return false;
      s = { kind: "idle" }; cached = null; fillCache = null; error = ""; return true;
    },
    overlay(env) {
      const px = 1/env.scale();
      if (s.kind === "idle") {
        const hit = pickOutline(env);
        return hit ? [...ringLines(hit.ring, px, HANDLE), ...ringHandles([hit.hit.point], px, false)] : [];
      }
      if (s.kind === "busy") return [];
      const r = result(env), lines: DrawLine[] = [];
      if (r) {
        r.shape.options.forEach((ring, i) => {
          const valid = mode === "reshape" ? r.shape.valid[i] : r.shape.partition;
          lines.push(...ringLines(ring, px, valid && keepSide(i) ? KEEP : DROP, true, keepSide(i) ? 2.5 : 1.5));
        });
        lines.push(...ringLines(r.boundary, px, PATH, false, 2), ...ringHandles([r.boundary[0], r.boundary[r.boundary.length-1]], px, false));
      } else if (s.kind === "path") lines.push(...ringLines(s.ring, px, HANDLE), ...ringLines([...s.pts, target(env).point], px, PATH, false, 2), ...ringHandles([s.start.point], px, false));
      return lines;
    },
    overlayFills(env): DrawFill[] {
      const r = result(env);
      if (!r || (mode !== "reshape" && !r.shape.partition)) return [];
      if (fillCache?.shape !== r.shape) {
        const triangles = r.shape.options.map(ring => {
          const flat = ring.flatMap(p => [p.x, p.y]);
          return earcut(flat).flatMap(i => [flat[2*i], flat[2*i+1]]);
        }) as [number[], number[]];
        fillCache = { shape: r.shape, triangles };
      }
      return r.shape.options.flatMap((_ring, i) => {
        if (!r.shape.valid[i]) return [];
        const c = keepSide(i) ? KEEP : DROP;
        return [{ tris: fillCache!.triangles[i], color: [c[0], c[1], c[2], 0.22] as Rgba }];
      });
    },
  };
}
