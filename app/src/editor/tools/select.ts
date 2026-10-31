import {revealLinkedComponent} from "../../core/crossProbe";
import { rotateWithConnections } from "../connectedRotation";
import { moveRejection } from "../connectedMove";
import { api } from "../../core/api";
import { highlightNet, navigateTo, report } from "../../core/actions";
import { toast } from "sonner";
import { filterActive, selectable, useSelectFilter } from "../../core/selectFilter";
import { cursorPoint } from "../../core/view";
import { useEditor, violatorModeOf } from "../../core/store";
import type { IdItem, Item, ItemId, ItemOf, Point, PcbDragPlan } from "../../core/types";
import { connectionPoints } from "../draw";
import { rotatePoint } from "../shapes";
import { constrain45, deleteVertex, editableRing, insertVertex, isValidRing, moveEdge, moveVertex, nearestOnRing, vertexAt, withRing } from "../polyedit";
import { commitRing, HANDLE_PX, ringHandles } from "./polygon";
import type { PointerInfo, Tool, ToolEnv } from "./types";

const DRAG_PX = 4;
const PICK_PX = 5;
const editCursor = (env: ToolEnv) => cursorPoint(env.raw(), env.doc().grid, env.doc().view?.snapToGrid !== false);

/** Selected items that may move: locked items stay put. */
function movableIds(env: ToolEnv, ids: ItemId[]): ItemId[] {
  const items = env.doc().items;
  const eligible=ids.filter(id=>{const it=items.get(id);return !!it&&selectable(env.doc().info.kind,it,env.doc());});
  const free = eligible.filter((id) => { const it = items.get(id)!; return !("locked" in it && it.locked); });
  if (!eligible.length && ids.length) useEditor.getState().setHint("No movable objects in the current view and selection filter");
  if (free.length < eligible.length) useEditor.getState().setHint(free.length ? `${eligible.length - free.length} locked item${eligible.length - free.length === 1 ? "" : "s"} left in place (unlock in Properties)` : "Locked: unlock the item in Properties to move it");
  return free;
}

type SelectionMode = "replace" | "add" | "toggle" | "subtract";
const selectionMode = (e: PointerInfo): SelectionMode => e.ctrl && e.shift ? "subtract" : e.ctrl ? "toggle" : e.shift ? "add" : "replace";
function combineSelection(current: ReadonlySet<ItemId>, ids: Iterable<ItemId>, mode: SelectionMode): Set<ItemId> {
  const next = new Set(mode === "replace" ? [] : current);
  for (const id of ids) {
    if (mode === "subtract" || (mode === "toggle" && next.has(id))) next.delete(id);
    else next.add(id);
  }
  return next;
}

type State =
  | { kind: "idle" }
  | { kind: "box"; sx: number; sy: number; mode: SelectionMode; shown: boolean }
  | { kind: "drag"; sx: number; sy: number; origin: Point; ids: ItemId[]; delta: Point; moved: boolean; attach: Attach | null; floating?: boolean; placing?:boolean; angle?: number; flipped?: boolean; tracks?: boolean | null; copper?: PcbDragPlan; partPending?: {key:string;promise:Promise<PcbDragPlan>}; copperKey?: string; acceptedKey?:string; copperError?: string; trackItems?:unknown; trackFootprints?:unknown; trackEpoch?:number; }
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
  return item && !("locked" in item && item.locked) && ring ? { id, item, ring } : null;
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
  let generation = 0;
  const blockedToast="connected-move-"+Math.random().toString(36).slice(2);
  let ignoreRelease = false;
  type BufferedInput={kind:"down"|"move"|"up";event:PointerInfo;raw:Point;cursor:Point};
  let buffered:BufferedInput[]=[];
  const buffer=(kind:BufferedInput["kind"],env:ToolEnv,event:PointerInfo)=>{const input={kind,event:{...event},raw:{...env.raw()},cursor:{...env.cursor()}};if(kind==="move"&&buffered.at(-1)?.kind==="move")buffered[buffered.length-1]=input;else if(buffered.length<32)buffered.push(input);};
  type Drag = Extract<State,{kind:"drag"}>;
  let planningTrack=false;let queuedTrack:(()=>void)|null=null;
  const previews=new WeakMap<Drag,Map<string,PcbDragPlan>>();
  const restorePreview=(env:ToolEnv,drag:Drag,key:string)=>{
    const plan=previews.get(drag)?.get(key);if(!plan)return false;
    drag.copper=plan;drag.acceptedKey=key;drag.copperKey=key;drag.copperError=undefined;drag.partPending=undefined;
    excluded=new Set([...plan.delete,...plan.replace.map(r=>r.id)]);queuedTrack=null;env.redraw(true);return true;
  };
  const acceptPreview=(env:ToolEnv,drag:Drag,key:string,plan:PcbDragPlan,currentKey:string)=>{
    let history=previews.get(drag);if(!history){history=new Map();previews.set(drag,history);}
    history.delete(key);history.set(key,plan);if(history.size>12)history.delete(history.keys().next().value!);
    // A late solve cannot displace a restored, exact-target preview.
    if(drag.acceptedKey===currentKey&&key!==currentKey)return;
    drag.copper=plan;drag.acceptedKey=key;excluded=new Set([...plan.delete,...plan.replace.map(r=>r.id)]);env.redraw(true);
  };
  const trackKey=(drag:Drag)=>[drag.delta.x,drag.delta.y,drag.angle??0,drag.flipped??false,violatorModeOf(useEditor.getState()),useEditor.getState().placement.cornerStyle].join("/");
  const trackSettings=(d:Drag)=>[d.angle??0,d.flipped??false,violatorModeOf(useEditor.getState()),useEditor.getState().placement.cornerStyle].join('/');
  const partSettings=(d:Drag)=>[trackSettings(d),useEditor.getState().placement.moveMode].join('/');
  const partKey=(drag:Drag)=>{
    const p=useEditor.getState().placement;
    return ['parts',drag.delta.x,drag.delta.y,drag.angle??0,drag.flipped??false,p.moveMode,violatorModeOf(useEditor.getState()),p.cornerStyle].join('/');
  };
  const requestParts=(env:ToolEnv,drag:Drag)=>{
    const doc=env.doc(),placement=useEditor.getState().placement;
    if(doc.info.kind!=='pcb'||placement.moveMode==='disconnect'||!drag.ids.some(id=>['footprint','via'].includes(doc.items.get(id)?.kind??'')))return;
    if(drag.trackItems!==doc.items||drag.trackFootprints!==doc.footprints||drag.trackEpoch!==doc.epoch){drag.copperError='Board changed during the move; cancel and start again.';return;}
    const key=partKey(drag);if(restorePreview(env,drag,key)||drag.copperKey===key)return;
    if(planningTrack){queuedTrack=()=>{if(s===drag)requestParts(env,drag);};return;}
    planningTrack=true;drag.copperKey=key;drag.copperError=undefined;
    if(!drag.copper)excluded=new Set(drag.ids);env.redraw(true);
    const items=doc.items,footprints=doc.footprints,epoch=doc.epoch,mode=violatorModeOf(useEditor.getState());
    const settings=partSettings(drag);
    const valid=()=>s===drag&&env.doc().items===items&&env.doc().footprints===footprints&&env.doc().epoch===epoch&&partSettings(drag)===settings;
    const current=()=>valid()&&partKey(drag)===key;
    const previewRequest=api.pcbDragFootprints(env.docId,{ids:drag.ids,delta:drag.delta,angle:drag.angle??0,flipped:drag.flipped??false,center:drag.origin,mode:placement.moveMode,routeMode:mode,corner:placement.cornerStyle});
    drag.partPending={key,promise:previewRequest};
    void previewRequest.then(result=>{if(!valid())return;const rejection=moveRejection(result,mode);if(rejection){if(current())drag.copperError=rejection;}else acceptPreview(env,drag,key,result,partKey(drag));env.redraw(true);})
      .catch(error=>{if(current()){drag.partPending=undefined;drag.copperError=String(error);env.redraw(true);}})
      .finally(()=>{planningTrack=false;const next=queuedTrack;queuedTrack=null;next?.();});
  };
  const requestTrack=(env:ToolEnv,drag:Drag)=>{
    const plan=trackPlan(env,drag);if(!plan){requestParts(env,drag);return;}if(drag.angle||drag.flipped)return;
    const key=trackKey(drag);if(drag.trackItems!==env.doc().items||drag.trackFootprints!==env.doc().footprints||drag.trackEpoch!==env.doc().epoch){drag.copperError="Board changed during track drag; cancel and start again.";return;}if(drag.copperKey===key)return;
    if(restorePreview(env,drag,key))return;
    if(planningTrack){queuedTrack=()=>{if(s===drag)requestTrack(env,drag);};return;}
    planningTrack=true;drag.copperKey=key;drag.copperError=undefined;if(!drag.copper)excluded=new Set();env.redraw(true);
    const doc=env.doc(),items=doc.items,footprints=doc.footprints,epoch=doc.epoch;
    const settings=trackSettings(drag);
    const valid=()=>s===drag&&env.doc().items===items&&env.doc().footprints===footprints&&env.doc().epoch===epoch&&trackSettings(drag)===settings;
    const current=()=>valid()&&trackKey(drag)===key;
    const previewRequest=api.pcbDragTracks(env.docId,trackRequest(env,drag));
    drag.partPending={key,promise:previewRequest};
    void previewRequest.then(result=>{
      if(!valid())return;const rejection=moveRejection(result,violatorModeOf(useEditor.getState()));if(rejection)throw new Error(rejection);acceptPreview(env,drag,key,result,trackKey(drag));
    }).catch(error=>{if(current()){drag.partPending=undefined;drag.copperError=String(error);useEditor.getState().setHint(String(error));env.redraw(true);}}).finally(()=>{planningTrack=false;const next=queuedTrack;queuedTrack=null;next?.();});
  };


  const far = (e: PointerInfo, sx: number, sy: number) => Math.hypot(e.sx - sx, e.sy - sy) > DRAG_PX;

  const tool: Tool = {
    id: "select",
    hint: (env) =>
      s.kind === "drag" && s.moved && env.doc().info.kind === "pcb"
        ? `Moving${s.angle ? ` · rotated ${s.angle}°` : ""}${s.flipped ? " · flipped to the other side" : ""}${s.ids.some((id) => ["footprint", "via"].includes(env.doc().items.get(id)?.kind ?? ""))
            ? ` · ${MOVE_MODE_LABEL[useEditor.getState().placement.moveMode]}${useEditor.getState().placement.moveMode === "preserveAngles" ? `, ${VIOLATOR_LABEL[violatorModeOf(useEditor.getState())]}${useEditor.getState().drcEnforce ? "" : " (design rules off)"}` : ""} (Tab / Shift+Tab)`
            : s.ids.every(id=>env.doc().items.get(id)?.kind==="track") ? ` · slide tracks · ${VIOLATOR_LABEL[violatorModeOf(useEditor.getState())]} (Shift+Tab)` : ""}${s.copperError?' · '+s.copperError:s.placing?' · placing':s.partPending&&s.acceptedKey!==s.partPending.key?' · checking route':''} · R rotates 90° · F flips components · right-drag pans · release / click to place · Esc cancels`
        : editedPolygon(env)
        ? "Polygon: drag a corner · drag a mid-point to add a corner · drag an edge to move it · Ctrl+click or Backspace removes a corner · Shift: 45° · E R reshapes"
        : s.kind === "drag" && s.moved && env.doc().info.kind === "schematic"
        ? "Moving · R rotates · X mirrors · release / click to place · Esc cancels"
        : "Click to select · Ctrl+click adds/removes · Drag empty space to box-select (→ inside, ← touching)" +
          (env.doc().info.kind === "pcb" ? " · M move · R rotate · X route" :
            env.doc().info.kind === "footprint" ? " · M move · R rotate · X mirror" : " · Drag selection to move · R rotates · X mirrors") +
          " · Esc cancel" +
          (env.doc().info.kind === "pcb" && filterActive(useSelectFilter.getState()) ? " · Selection filter active" : "") +
          (env.doc().info.kind === "pcb" ? (env.doc().marked.size ? " · Net highlighted (Esc clears)" : " · Alt+click or ` highlights a net · I selects its copper") : ""),
    editing: () => s.kind !== "idle",
    reset() {
      buffered=[];
      generation++;
      s = { kind: "idle" };
      excluded = new Set();
    },
    down(env, e) {
      if(s.kind==="drag"&&s.placing){if(s.tracks){buffer("down",env,e);const d=env.doc();const hit=env.scene.pick(env.raw(),PICK_PX/env.scale(),i=>selectable(d.info.kind,i,d));if(hit!==null)env.select([hit]);}return;}
      ignoreRelease = false;
      generation++;
      if (s.kind === "drag" && s.floating) {
        tool.up?.(env, e);
        ignoreRelease = true;
        return;
      }
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
            s = { kind: "edge", ...poly, edge: hit.edge, origin: editCursor(env), ring2: poly.ring, sx: e.sx, sy: e.sy, moved: false };
          }
          excluded = new Set([poly.id]);
          return;
        }
      }
      const kind = doc.info.kind;
      if (e.alt && kind === "pcb") {
        const at = env.raw();
        void import("../../core/pcbActions").then((m) => m.highlightNetAt(env.docId, at));
        return;
      }
      const hit = env.scene.pick(p, PICK_PX / env.scale(), (i) => selectable(kind, i, doc));
      if (hit === null) {
        s = { kind: "box", sx: e.sx, sy: e.sy, mode: selectionMode(e), shown: false };
        return;
      }
      if (e.alt && doc.info.kind === "schematic") {
        void highlightNet(env.docId, hit);
        return;
      }
      if (e.shift || e.ctrl) {
        env.select(combineSelection(doc.selection, [hit], selectionMode(e)));
        return;
      }
      if (!doc.selection.has(hit)) env.select([hit]);
      const ids = movableIds(env, [...env.doc().selection]);
      if (ids.length) s = { kind: "drag", sx: e.sx, sy: e.sy, origin: editCursor(env), ids, delta: { x: 0, y: 0 }, moved: false, attach: null };
    },
    move(env, e) {
      if(s.kind==="drag"&&s.placing){if(s.tracks&&buffered.length)buffer("move",env,e);return;}
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
        if(s.moved)requestTrack(env,s);
        if (s.moved && (!wasMoved || !s.attach)) {
          s.attach = attachedTo(env, s.ids);
          const plan = trackPlan(env, s);
          excluded = new Set([...s.ids, ...s.attach.follow, ...s.attach.stretch.map((r) => r.id)]);
          if(env.doc().info.kind==='pcb'&&(plan||useEditor.getState().placement.moveMode!=='disconnect'&&s.ids.some(id=>['footprint','via'].includes(env.doc().items.get(id)?.kind??''))))excluded=new Set(s.copper?[...s.copper.delete,...s.copper.replace.map(r=>r.id)]:plan?[]:s.ids);
          env.redraw(true);
        }
      }
    },
    up(env, e) {
      if(s.kind==="drag"&&s.placing){if(s.tracks&&buffered.length)buffer("up",env,e);return;}
      if (ignoreRelease) { ignoreRelease = false; return; }
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
          if (done.mode === "replace") {
            env.select([]);
            useEditor.getState().patchDoc(env.docId, { marked: new Set() });
          }
          return;
        }
        const a = env.toWorld(done.sx, done.sy);
        const b = env.toWorld(e.sx, e.sy);
        const kind = env.doc().info.kind;
        const rect = { minX: Math.min(a.x, b.x), minY: Math.min(a.y, b.y), maxX: Math.max(a.x, b.x), maxY: Math.max(a.y, b.y) };
        const ids = env.scene.inRect(rect, e.sx < done.sx, (i) => selectable(kind, i, env.doc()));
        env.select(combineSelection(env.doc().selection, ids, done.mode));
        if (!ids.length && kind === "pcb" && filterActive(useSelectFilter.getState())) {
          const excluded = env.scene.inRect(rect, e.sx < done.sx, (i) => i.kind !== "board").length;
          if (excluded) toast.info(`Nothing selected: ${excluded} object${excluded === 1 ? "" : "s"} in the box ${excluded === 1 ? "is" : "are"} excluded by the selection filter.`, {
            action: { label: "Reset filter", onClick: () => useSelectFilter.getState().reset() },
          });
        }
      } else if (done.kind === "drag") {
        const moved = done.moved && (done.delta.x || done.delta.y || done.angle || done.flipped);
        const finish = () => {
          if(s.kind==="drag"&&s.placing)s={kind:"idle"};
          excluded = new Set();
          env.redraw(true);
          const inputs=buffered;buffered=[];
          for(const input of inputs){const currentEnv={...env,raw:()=>input.raw,cursor:()=>input.cursor};tool[input.kind]?.(currentEnv,input.event);}
        };
        if (!moved) return finish();
        const { x: dx, y: dy } = done.delta;
        const plan = !done.angle && !done.flipped ? trackPlan(env, done) : null;
        if (env.doc().info.kind === "pcb" && done.ids.some(id => ["footprint","via"].includes(env.doc().items.get(id)?.kind??""))) {
          if(done.trackItems!==undefined&&(done.trackItems!==env.doc().items||done.trackFootprints!==env.doc().footprints||done.trackEpoch!==env.doc().epoch)){useEditor.getState().setHint('Board changed during the move; cancel and start again.');finish();return;}
          s={...done,placing:true};
          // Keep the proposed placement visible while native planning completes.
          const pl = useEditor.getState().placement;
          const at = done.delta;
          const routeMode = violatorModeOf(useEditor.getState());
          const originalItems = env.doc().items;
          const originalFootprints = env.doc().footprints, originalEpoch = env.doc().epoch;
          const token = generation;
          let retrying = false;
          const retry = (message?:string) => {
            if (token !== generation || env.doc()?.items !== originalItems || env.doc()?.footprints !== originalFootprints || env.doc()?.epoch !== originalEpoch) return;
            s = { ...done, floating: true, placing:false, copper:undefined, copperKey:undefined, copperError:message, partPending:undefined, tracks: undefined };
            retrying = true;
            excluded = new Set(s.ids);
            env.redraw(true);
          };
          const reviewed=done.copper&&done.acceptedKey===partKey(done)?done.copper:null;
          void (reviewed?Promise.resolve(reviewed):done.partPending?.key===partKey(done)?done.partPending.promise:api.pcbDragFootprints(env.docId, { ids: done.ids, delta: at, angle: done.angle ?? 0, flipped: done.flipped ?? false, center: done.origin, mode: pl.moveMode, routeMode, corner: pl.cornerStyle }))
            .then(async (r) => {
              if (token !== generation) return;
              if (env.doc()?.items !== originalItems || env.doc()?.footprints !== originalFootprints || env.doc()?.epoch !== originalEpoch) { useEditor.getState().setHint("Board changed while planning the move; try again."); return; }
              const current = useEditor.getState();
              if (current.placement.moveMode !== pl.moveMode || violatorModeOf(current) !== routeMode || current.placement.cornerStyle !== pl.cornerStyle) {
                useEditor.getState().setHint("Move settings changed while planning; choose a position again."); retry(); return;
              }
              const rejection = moveRejection(r, routeMode);
              if (rejection) { toast.warning(rejection,{id:blockedToast}); useEditor.getState().setHint(rejection); retry(rejection); return; }
              const ok = await env.exec({ op: "route", delete: r.delete, add: r.add, replace: r.replace });
              if (!ok) retry();
              if (ok && r.blocked) toast.warning(`${r.blocked} connection${r.blocked === 1 ? "" : "s"} could not be re-routed cleanly; joined directly (run DRC to see the conflicts).`);
            })
            .catch(error=>{report(error);retry(String(error));})
            .finally(() => { if (token === generation && !retrying) finish(); });
          return;
        }
        if (plan) {
          if(done.trackItems!==env.doc().items||done.trackFootprints!==env.doc().footprints||done.trackEpoch!==env.doc().epoch){useEditor.getState().setHint("Board changed during track drag; start again.");finish();return;}
          s={...done,placing:true};
          const doc=env.doc(),items=doc.items,footprints=doc.footprints,epoch=doc.epoch,token=generation;
          const mode=violatorModeOf(useEditor.getState()),corner=useEditor.getState().placement.cornerStyle;
          const retry=(message?:string)=>{if(message)useEditor.getState().setHint(message);};
          // Retain exactly the checked geometry shown while a later target is blocked.
          const fallback=done.copper&&done.acceptedKey?.endsWith('/'+trackSettings(done))?done.copper:null;
          const commit=async(result:PcbDragPlan)=>{
            if(token!==generation||env.doc().items!==items||env.doc().footprints!==footprints||env.doc().epoch!==epoch)return;
            if(violatorModeOf(useEditor.getState())!==mode||useEditor.getState().placement.cornerStyle!==corner){retry();return;}
            const rejection=moveRejection(result,mode);if(rejection)throw new Error(rejection);
            if(await env.exec({op:"route",delete:result.delete,add:result.add,replace:result.replace})){if(done.acceptedKey!==trackKey(done))useEditor.getState().setHint("Placed the last valid track position; the cursor position was blocked or still being checked.");}
          };
          const reviewed=fallback;
          void (reviewed?Promise.resolve(reviewed):done.partPending?.key===trackKey(done)?done.partPending.promise:api.pcbDragTracks(env.docId,trackRequest(env,done))).then(commit).catch(async error=>{
            useEditor.getState().setHint(String(error));toast.warning(String(error),{id:blockedToast});
          }).finally(()=>{if(token===generation)finish();});
          return;
        }
        if (done.angle || done.flipped) {
          const items = done.ids.flatMap(id => { const item=env.doc().items.get(id); return item ? [{id,item:translate(dragTransform(item,done),dx,dy)}] : []; });
          void env.exec({op:"replaceMany",items}).then(finish);
          return;
        }
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
      const hit = env.scene.pick(env.raw(), PICK_PX / env.scale(),item=>selectable(env.doc().info.kind,item,env.doc()));
      const item = hit !== null ? env.doc().items.get(hit) : undefined;
      if (item?.kind === "sheet") void navigateTo({ sheet: item.file });
      else if(item?.kind==="part"&&hit!==null)void revealLinkedComponent(env.docId,hit);
    },
    pickUp(env, list) {
      generation++;
      const ids = movableIds(env, list.filter((id) => env.doc().items.has(id)));
      const b = env.scene.bounds(ids);
      if (!ids.length || !b) return;
      env.select(ids);
      // The cluster's centre on the grid follows the cursor, so the parts keep their grid alignment.
      const g = env.doc().grid || 1;
      const origin = { x: Math.round((b.minX + b.maxX) / 2 / g) * g, y: Math.round((b.minY + b.maxY) / 2 / g) * g };
      const c = editCursor(env);
      s = { kind: "drag", sx: 0, sy: 0, origin, ids, delta: { x: c.x - origin.x, y: c.y - origin.y }, moved: true, attach: attachedTo(env, ids), floating: true };
      excluded = new Set(ids);
      env.redraw(true);
    },
    key(env, key, event) {
      if(s.kind==="drag"&&s.placing)return true;
      const doc = env.doc();
      const board = doc.info.kind === "pcb" || doc.info.kind === "footprint";
      // ` highlights the net under the cursor; I selects its tracks and vias.
      if (doc.info.kind === "pcb" && s.kind === "idle" && (key === "`" || key.toLowerCase() === "i") && !event.ctrlKey && !event.altKey) {
        const at = env.raw();
        void import("../../core/pcbActions").then((m) => (key === "`" ? m.highlightNetAt(env.docId, at) : m.selectNetCopperAt(env.docId, at)));
        return true;
      }
      if (board && s.kind === "idle" && ["m", "r"].includes(key.toLowerCase())) {
        generation++;
        if (!doc.selection.size) {
          const hit = env.scene.pick(env.raw(), PICK_PX / env.scale(), item => selectable(doc.info.kind, item, doc));
          if (hit === null) return false;
          env.select([hit]);
        }
        const ids = movableIds(env, [...env.doc().selection]);
        if (!ids.length) return true;
        const item = ids.length === 1 ? doc.items.get(ids[0]) : undefined;
        const origin = item?.kind === "footprint" ? item.pos : editCursor(env);
        if (key.toLowerCase() === "r") {
          const token=generation;
          void rotateWithConnections(env.docId,ids,origin,event.shiftKey?-90:90,cmd=>env.exec(cmd),()=>env.doc(),()=>generation===token&&s.kind==="idle").catch(report);
        } else {
          const c = editCursor(env);
          s = { kind: "drag", sx: 0, sy: 0, origin, ids, delta: { x: c.x-origin.x, y: c.y-origin.y }, moved: true, attach: attachedTo(env, ids), floating: true };
          excluded = new Set(trackPlan(env,s)?[]:ids);
          env.redraw(true);
        }
        return true;
      }
      if (board && s.kind === "drag" && ["f","l"].includes(key.toLowerCase()) && s.ids.every(id=>doc.items.get(id)?.kind === "footprint")) {
        s.flipped = !s.flipped; s.moved = true; s.copper=undefined; s.copperKey=undefined; excluded=new Set(s.ids); requestTrack(env,s); env.redraw(true); return true;
      }
      const poly = s.kind === "idle" ? editedPolygon(env) : null;
      if (poly && key === "Backspace") {
        const vi = vertexAt(poly.ring, env.raw(), HANDLE_PX / env.scale());
        if (vi >= 0 && poly.ring.length > 3) {
          void commitRing(env, poly.id, poly.item, deleteVertex(poly.ring, vi));
          return true;
        }
      }
      // Tab: move mode (disconnect / free / preserve angles); Shift+Tab: what happens to other nets in the way.
      if (s.kind === "drag" && s.moved && key === "Tab" && doc.info.kind === "pcb") {
        const pl = useEditor.getState().placement;
        if(!event.shiftKey&&trackPlan(env,s))return true;
        if (event.shiftKey) {
          const order = ["shove", "walkaround", "ignore"] as const;
          env.setPlacement({ moveViolators: order[(order.indexOf(pl.moveViolators) + 1) % order.length] });
        } else {
          const order = ["preserveAngles", "free", "disconnect"] as const;
          env.setPlacement({ moveMode: order[(order.indexOf(pl.moveMode) + 1) % order.length] });
          // The attached tracks depend on the mode; show them again (or release them) right away.
          excluded = new Set(s.ids);
        }
        requestTrack(env,s);
        env.redraw(true);
        return true;
      }
      if (s.kind === "drag" && s.moved && (key.toLowerCase() === "r" || key === "x" || key === "X")) {
        if(board&&trackPlan(env,s)){useEditor.getState().setHint("Drag a track or corner to reshape its connections.");return true;}
        if (board && (key.toLowerCase() === "r")) {
          s.angle = ((s.angle ?? 0) + (event.shiftKey ? -90 : 90) + 360) % 360;
          s.copper=undefined;s.copperKey=undefined;excluded=new Set(s.ids);requestTrack(env,s);
          env.redraw(true);
          return true;
        }
        // Rotate/mirror the dragged items in place around the grab point.
        const center = { x: s.origin.x, y: s.origin.y };
        const ids = s.ids;
        if (key.toLowerCase() === "r") void env.exec({ op: "rotate", ids, center, angle: event.shiftKey ? -90 : 90 });
        else void env.exec({ op: "mirror", ids, center });
        // Pins moved: work out attached wires again on the next pointer move.
        s.attach = null;
        return true;
      }
      if (!doc.selection.size) return false;
      if (key.toLowerCase() === "r") {
        const token=++generation;
        void rotateWithConnections(env.docId,[...doc.selection],env.cursor(),90,cmd=>env.exec(cmd),()=>env.doc(),()=>generation===token&&s.kind==="idle").catch(report);
        return true;
      }
      if (key === "x" || key === "X" || ((key === "l" || key === "L" || key.toLowerCase() === "f") && doc.info.kind === "pcb")) {
        void env.exec({ op: "mirror", ids: [...doc.selection], center: env.cursor() });
        return true;
      }
      return false;
    },
    cancel(env, source) {
      buffered=[];
      generation++;
      if (s.kind === "idle") {
        // Esc first clears a net highlight, then (next press) the selection as before;
        // a right-click leaves it and opens the context menu.
        if (source === "mouse" || env.doc().info.kind !== "pcb" || !env.doc().marked.size) return false;
        useEditor.getState().patchDoc(env.docId, { marked: new Set() });
        return true;
      }
      if (s.kind === "box") env.setBox(null);
      generation++;
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
      const drag = s;
      const plan = !s.angle && !s.flipped ? trackPlan(env, s) : null;
      const nativeParts=env.doc().info.kind==='pcb'&&useEditor.getState().placement.moveMode!=='disconnect'&&s.ids.some(id=>['footprint','via'].includes(items.get(id)?.kind??''));
      if(plan||nativeParts){
        if(s.trackItems!==items||s.trackFootprints!==env.doc().footprints||s.trackEpoch!==env.doc().epoch)return [];
        return s.copper?[...s.copper.replace.map(r=>r.item),...s.copper.add]:plan?[]:s.ids.flatMap(id=>{const item=items.get(id);return item?[translate(dragTransform(item,drag),dx,dy)]:[];});
      }
      const attach = s.attach ?? { stretch: [], follow: [] };
      const moved = [...s.ids, ...attach.follow]
        .map((id) => items.get(id))
        .filter((i): i is Item => !!i)
        .map((i) => translate(dragTransform(i, drag), dx, dy));
      return [...moved, ...attach.stretch.map((r) => stretched(r, dx, dy))];
    },
    excluded: () => excluded,
    // Corner and edge drags of a polygon snap to pads, track ends and corners.
    snaps: () => s.kind === "vertex" || s.kind === "edge",
    gridSnap: () => s.kind === "drag" || s.kind === "vertex" || s.kind === "edge",
    overlay(env) {
      const px = 1 / env.scale();
      if (s.kind === "vertex" || s.kind === "edge") return ringHandles(s.ring2, px);
      if (s.kind === "drag" && s.moved) {
        if(!s.copperError)return [];
        const p=env.cursor(),r=5*px,color:[number,number,number,number]=[1,.3,.25,.9];
        return [{a:{x:p.x-r,y:p.y-r},b:{x:p.x+r,y:p.y+r},width:1.5*px,color},{a:{x:p.x-r,y:p.y+r},b:{x:p.x+r,y:p.y-r},width:1.5*px,color}];
      }
      const poly = editedPolygon(env);
      return poly ? ringHandles(poly.ring, px) : [];
    },
  };
  return tool;
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
    case "dimension":
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

const MOVE_MODE_LABEL = { disconnect: "disconnect tracks", free: "free (stretch)", preserveAngles: "preserve angles" } as const;
const VIOLATOR_LABEL = { ignore: "ignore violators", walkaround: "avoid violators", shove: "push violators" } as const;

/** Track-only edits are planned by the native connected router. */
function trackPlan(env: ToolEnv, d: {ids:ItemId[];tracks?:boolean|null;trackItems?:unknown;trackFootprints?:unknown;trackEpoch?:number}):boolean|null{
 if(d.tracks===undefined){const doc=env.doc();d.tracks=doc.info.kind==='pcb'&&d.ids.length>0&&d.ids.every(id=>['track','arcTrack'].includes(doc.items.get(id)?.kind??''))?true:null;d.trackItems=doc.items;d.trackFootprints=doc.footprints;d.trackEpoch=doc.epoch;}
 return d.tracks;
}
function trackRequest(env:ToolEnv,d:{ids:ItemId[];delta:Point;origin:Point}){
 return {ids:d.ids,delta:d.delta,point:d.origin,mode:violatorModeOf(useEditor.getState()),corner:useEditor.getState().placement.cornerStyle,tolerance:Math.round(PICK_PX/env.scale())};
}

/** Temporary rotation and side flip of a drag, applied in that order about the grab point. */
function dragTransform(item: Item, d: { origin: Point; angle?: number; flipped?: boolean }): Item {
  const rotated = d.angle ? rotatePreview(item, d.origin, d.angle) : item;
  return d.flipped ? flipPreview(rotated, d.origin) : rotated;
}

/** Flips a footprint to the other board side about `center`, as the Mirror command (F / L) does. */
function flipPreview(item: Item, center: Point): Item {
  if (item.kind !== "footprint") return item;
  return { ...item, pos: { x: 2 * center.x - item.pos.x, y: item.pos.y }, rotation: ((-item.rotation % 360) + 360) % 360, side: item.side === "top" ? "bottom" : "top" };
}

/** Temporary board rotation; committed together with the move and cancelled by Esc. */
function rotatePreview(item: Item, center: Point, angle: number): Item {
  const p=(point:Point)=>rotatePoint(point,center,angle);
  switch(item.kind) {
    case 'line': case 'wire': case 'bus': case 'rect': case 'track': return {...item,a:p(item.a),b:p(item.b)};
    case 'arc': case 'arcTrack': return {...item,center:p(item.center),startAngle:(item.startAngle+angle+360)%360};
    case 'circle':return {...item,center:p(item.center)};
    case 'polyline':return {...item,points:item.points.map(p)};
    case 'zone':return {...item,outline:item.outline.map(p),fill:item.fill.map(f=>({outer:f.outer.map(p),holes:f.holes.map(h=>h.map(p))}))};
    case 'dimension':return {...item,a:p(item.a),b:p(item.b),angle:(item.angle+angle+360)%360};
    case 'board':return item;
    default:return {...item,pos:p(item.pos),...('rotation' in item ? {rotation:(item.rotation+angle+360)%360}: {})} as Item;
  }
}
