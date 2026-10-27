// Interactive routing: the backend router computes each preview on mouse move;
// clicks commit segments. "*" drops a via and switches copper layer, Space
// flips the 45° posture, Shift+R cycles walkaround → shove → ignore.

import { api } from "../../core/api";
import { report } from "../../core/actions";
import { L } from "../../core/layers";
import { useEditor } from "../../core/store";
import type { BoardItem, Item, ItemOf, Point, RouteMode, TrackItem } from "../../core/types";
import { NM_PER_MM } from "../../core/units";
import { constrain45 } from "../polyedit";
import { samePoint } from "../shapes";
import { place } from "../draw";
import { ringHandles, ringLines } from "./polygon";
import type { Tool, ToolEnv } from "./types";

const MODES: RouteMode[] = ["walkaround", "shove", "ignore"];

export function boardSettings(env: ToolEnv): BoardItem | undefined {
  for (const it of env.doc().items.values()) if (it.kind === "board") return it;
  return undefined;
}

export function classOf(board: BoardItem | undefined, net: string) {
  return board?.netClasses.find((c) => c.nets.includes(net)) ?? board?.netClasses[0];
}

/** Copper layers top → bottom from the board stack. */
export function copperLayers(board: BoardItem | undefined): number[] {
  const cu = board?.stackup.layers.filter((l) => l.kind === "copper" && l.layer !== null).map((l) => l.layer as number);
  return cu?.length ? cu : [L.topCopper, L.bottomCopper];
}

function trackWidth(env: ToolEnv, net: string): number {
  return env.placement().trackWidth ?? classOf(boardSettings(env), net)?.trackWidth ?? Math.round(0.25 * NM_PER_MM);
}

interface Session {
  net: string;
  layer: number;
  from: Point;
  preview: TrackItem[];
  /** Other-net tracks pushed aside by the shove router (replacements keyed by original id). */
  moved: { id: number; item: Item }[];
  reached: boolean;
  /** Footprint pad / copper end points of the net (route ends there automatically). */
  targets: Point[];
  message: string;
  blockedAt: Point | null;
}

export function routingAnchor(env: ToolEnv, point: Point, hit: [number, string] | null): Point {
  const item = hit ? env.doc().items.get(hit[0]) : undefined;
  if (item?.kind === "via" || item?.kind === "pad") return item.pos;
  if (item?.kind === "footprint") {
    const fp = env.doc().footprints[item.footprint];
    const pads = fp?.items.filter((p): p is ItemOf<"pad"> => p.kind === "pad" && (item.padNets[p.number] ?? "") === hit![1]);
    const centers = pads?.map(p => place(p.pos, { pos: item.pos, rotation: item.rotation, mirror: item.side === "bottom" })) ?? [];
    const center = centers.sort((a,b) => Math.hypot(a.x-point.x,a.y-point.y)-Math.hypot(b.x-point.x,b.y-point.y))[0] ?? point;
    return {x:Math.round(center.x),y:Math.round(center.y)};
  }
  if (item?.kind === "track") {
    const dx=item.b.x-item.a.x, dy=item.b.y-item.a.y;
    const t=Math.max(0,Math.min(1,((point.x-item.a.x)*dx+(point.y-item.a.y)*dy)/(dx*dx+dy*dy || 1)));
    return {x:Math.round(item.a.x+t*dx),y:Math.round(item.a.y+t*dy)};
  }
  return point;
}

export function routeTool(pair = false): Tool {
  let s: Session | null = null;
  let flip = false;
  let seq = 0;
  let inflight = false;
  let pending: Point | null = null;
  let starting = false;
  let committing = false;
  let previewSettled: Promise<void> = Promise.resolve();

  const requestPreview = (env: ToolEnv, to: Point) => {
    if (!s) return;
    if (inflight) {
      pending = to;
      return;
    }
    inflight = true;
    const my = ++seq;
    const session = s;
    const req = {
      net: session.net,
      layer: session.layer,
      start: session.from,
      end: to,
      width: trackWidth(env, session.net),
      mode: useEditor.getState().placement.routeMode,
      corner: useEditor.getState().placement.cornerStyle,
      ignore: [],
      postureFlip: flip,
    };
    const done = () => {
      inflight = false;
      if (pending) {
        const p = pending;
        pending = null;
        requestPreview(env, p);
      }
    };
    const run = pair ? api.pcbRoutePair(env.docId, req, negativeOf(env, session.net), diffGap(env, session.net)).then(([pos, neg]) => ({ ...pos, tracks: [...pos.tracks, ...neg.tracks] })) : api.pcbRoute(env.docId, req);
    previewSettled = run
      .then((r) => {
        if (my !== seq || s !== session) return;
        const hadMoved = session.moved.length > 0;
        session.preview = r.tracks;
        session.moved = r.moved;
        session.reached = r.reached;
        session.message = r.message;
        session.blockedAt = r.blockedAt;
        env.redraw(hadMoved || r.moved.length > 0);
      })
      .catch((error) => {
        if (s !== session || my !== seq) return;
        session.preview = [];
        session.message = String(error);
        session.reached = false;
        report(error);
        env.redraw();
      })
      .finally(done);
  };

  const netTargets = (env: ToolEnv, net: string): Point[] => {
    if (!net) return [];
    const doc = env.doc();
    const out: Point[] = [];
    for (const it of doc.items.values()) {
      if (it.kind !== "footprint") continue;
      const geom = doc.footprints[it.footprint];
      if (!geom) continue;
      for (const p of geom.items) {
        if (p.kind !== "pad" || it.padNets[p.number] !== net) continue;
        const pl = { pos: it.pos, rotation: it.rotation, mirror: it.side === "bottom" };
        const m = pl.mirror ? { x: -p.pos.x, y: p.pos.y } : p.pos;
        const a = (pl.rotation * Math.PI) / 180;
        out.push({ x: Math.round(pl.pos.x + m.x * Math.cos(a) - m.y * Math.sin(a)), y: Math.round(pl.pos.y + m.x * Math.sin(a) + m.y * Math.cos(a)) });
      }
    }
    return out;
  };

  const commit = async (env: ToolEnv): Promise<boolean> => {
    if (!s || committing) return false;
    const session = s;
    committing = true;
    try {
      requestPreview(env, env.cursor());
      // Commit the clicked endpoint, never an older asynchronous mouse preview.
      while (inflight || pending) await previewSettled;
      if (s !== session || !session.preview.length) return false;
      const tracks = session.preview;
      const moved = session.moved;
      seq++;
      pending = null;
      const ok = await env.exec({ op: "route", delete: [...new Set(moved.map(m => m.id))], add: [...moved.map(m => m.item), ...tracks] });
      if (!ok || s !== session) return false;
      const last = tracks[pair ? Math.floor(tracks.length / 2) - 1 : tracks.length - 1] ?? tracks[tracks.length - 1];
      session.from = last.b;
      session.preview = [];
      session.moved = [];
      session.message = "";
      session.blockedAt = null;
      return true;
    } finally { committing = false; }
  };

  const finish = () => {
    s = null;
    pending = null;
    starting = false;
    seq++;
  };

  return {
    id: pair ? "diffPair" : "route",
    hint: () =>
      s
        ? `Routing ${s.net || "(no net)"} on ${s.layer === L.bottomCopper ? "bottom" : s.layer === L.topCopper ? "top" : "inner"} · click to commit · * via + layer · / flip corner · Shift+R mode (${useEditor.getState().placement.routeMode}) · Esc to select${s.message ? ` · ${s.reached ? "" : "Blocked: "}${s.message}` : ""}`
        : `Click a pad or track to start ${pair ? "a differential pair" : "routing"}`,
    reset: finish,
    down(env) {
      const p = env.cursor();
      if (!s) {
        if (starting) return;
        starting = true;
        const generation = ++seq;
        const layer = env.doc().activeLayer;
        const copper = copperLayers(boardSettings(env));
        const routeLayer = copper.includes(layer) ? layer : L.topCopper;
        const mouse = env.raw();
        const raw = {x:Math.round(mouse.x),y:Math.round(mouse.y)};
        void api.pcbObjectAt(env.docId, raw, routeLayer)
          .then(async hit => hit ?? (samePoint(raw, p) ? null : await api.pcbObjectAt(env.docId, p, routeLayer)))
          .then(hit => {
            if (generation !== seq) return;
            starting = false;
            const net = hit?.[1] ?? useEditor.getState().placement.net;
            const from = routingAnchor(env, p, hit);
            s = { net, layer: routeLayer, from, preview: [], moved: [], reached: true, targets: netTargets(env, net), message: "", blockedAt: null };
            requestPreview(env, env.cursor());
            env.redraw();
          })
          .catch(error => { if (generation === seq) { starting = false; report(error); } });
        return;
      }
      void commit(env).then((ok) => {
        if (ok && s && s.targets.some((t) => samePoint(t, p))) finish();
        env.redraw();
      });
    },
    move(env) {
      if (s && !committing) requestPreview(env, env.cursor());
    },
    dblclick(env) {
      void commit(env).then(() => {
        finish();
        env.redraw();
      });
    },
    key(env, key, e) {
      if (!s) return false;
      if (key === " " || key === "/") {
        flip = !flip;
        requestPreview(env, env.cursor());
        return true;
      }
      if ((key === "R" || key === "r") && e.shiftKey) {
        const m = useEditor.getState().placement.routeMode;
        env.setPlacement({ routeMode: MODES[(MODES.indexOf(m) + 1) % MODES.length] });
        requestPreview(env, env.cursor());
        return true;
      }
      if (key === "*" || key === "v" || key === "V") {
        const session = s;
        void (async () => {
          await commit(env);
          if (s !== session) return;
          const via = await api.pcbVia(env.docId, session.net, session.from);
          if (s !== session) return;
          if (!await env.exec({ op: "route", delete: [], add: [via] }) || s !== session) return;
          const cu = copperLayers(boardSettings(env));
          const i = cu.indexOf(session.layer);
          session.layer = cu[(i + 1) % cu.length];
          useEditor.getState().patchDoc(env.docId, { activeLayer: session.layer });
          requestPreview(env, env.cursor());
        })().catch(report);
        return true;
      }
      return false;
    },
    cancel() {
      if (!s && !starting) return false;
      finish();
      return true;
    },
    preview: () => (s ? [...s.moved.map((m) => m.item), ...s.preview] : []),
    excluded: () => new Set(s?.moved.map((m) => m.id) ?? []),
    overlay: env => {
      if (!s) return [];
      const width = 1.5 / env.scale();
      const lines = s.preview.map(t => ({ a: t.a, b: t.b, width: t.width, color: [1, 0.85, 0.3, 0.95] as [number, number, number, number] }));
      const mark = (p: Point, color: [number,number,number,number]) => {
        const r=5/env.scale();
        lines.push({a:{x:p.x-r,y:p.y},b:{x:p.x+r,y:p.y},width,color});
        lines.push({a:{x:p.x,y:p.y-r},b:{x:p.x,y:p.y+r},width,color});
      };
      mark(s.from,[0.3,1,0.65,1]);
      if (!s.reached && s.blockedAt) mark(s.blockedAt,[1,0.3,0.25,1]);
      return lines;
    },
  };
}

function negativeOf(env: ToolEnv, net: string): string {
  const pairs = boardSettings(env)?.diffPairs ?? [];
  const p = pairs.find((d) => d.positive === net || d.negative === net);
  if (p) return p.positive === net ? p.negative : p.positive;
  // Common naming conventions: USB_P/USB_N, D+/D-, CLK_P/CLK_N.
  for (const [a, b] of [
    ["_P", "_N"],
    ["+", "-"],
    ["_DP", "_DN"],
    ["P", "N"],
  ]) {
    if (net.endsWith(a)) return net.slice(0, -a.length) + b;
    if (net.endsWith(b)) return net.slice(0, -b.length) + a;
  }
  return net;
}

function diffGap(env: ToolEnv, net: string): number {
  return classOf(boardSettings(env), net)?.diffPairGap || Math.round(0.2 * NM_PER_MM);
}

// ---------- polygon-point tools (zones, keep-outs, board outline) ----------

export function polygonTool(id: "zone" | "keepout" | "outline"): Tool {
  let pts: Point[] = [];
  let shift = false;
  /** Next corner: grid cursor, 45° constrained with Shift, or the first corner when close to it. */
  const next = (env: ToolEnv): { p: Point; closes: boolean } => {
    const c = env.cursor();
    if (pts.length >= 3) {
      const r = env.raw();
      if (Math.hypot(r.x - pts[0].x, r.y - pts[0].y) <= 8 / env.scale()) return { p: pts[0], closes: true };
    }
    return { p: shift && pts.length ? constrain45(pts[pts.length - 1], c) : c, closes: false };
  };
  const build = (env: ToolEnv, points: Point[]): Item | null => {
    if (points.length < 2) return null;
    const pl = env.placement();
    const layer = env.doc().activeLayer;
    if (id === "outline") return { kind: "polyline", points, closed: points.length > 2, width: Math.round(0.1 * NM_PER_MM), layer: L.boardOutline, fill: "none" };
    const zone: ItemOf<"zone"> = {
      kind: "zone",
      outline: points,
      layer: layer === L.multi ? L.topCopper : layer,
      net: id === "keepout" ? "" : pl.net,
      name: "",
      priority: pl.zonePriority,
      // The pour keeps the larger of this and the clearance rules.
      clearance: id === "keepout" ? 0 : Math.round(0.3 * NM_PER_MM),
      minWidth: Math.round(0.2 * NM_PER_MM),
      connect: "thermal",
      thermalGap: Math.round(0.3 * NM_PER_MM),
      spokeWidth: Math.round(0.3 * NM_PER_MM),
      keepout: id === "keepout",
      removeIslands: true,
      fill: [],
    };
    return zone;
  };
  const close = (env: ToolEnv) => {
    const corners = pts;
    const item = corners.length >= 3 ? build(env, corners) : null;
    pts = [];
    if (!item) return;
    void (async () => {
      if (item.kind === "zone" && !item.keepout && !item.net) item.net = await pourNet(env, corners[0], item.layer);
      return env.exec({ op: "add", items: [item] });
    })().then(async (ok) => {
      if (!ok || item.kind !== "zone" || item.keepout) return;
      // Pour right away so the user sees the copper.
      const { repour } = await import("../../core/pcbActions");
      await repour(env.docId);
    });
  };
  const label = id === "zone" ? "copper pour" : id === "keepout" ? "keep-out area" : "board outline";
  /** No net chosen: take the copper under the first corner, else GND. */
  const pourNet = async (env: ToolEnv, at: Point, layer: number): Promise<string> => {
    try {
      const hit = await api.pcbObjectAt(env.docId, at, layer);
      if (hit?.[1]) return hit[1];
      const nets = await api.pcbNets(env.docId);
      const gnd = nets.find((n) => n.toUpperCase() === "GND") ?? nets.find((n) => /^(A|D|P)?GND/i.test(n));
      if (gnd) {
        useEditor.getState().log("info", `New pour connected to ${gnd} — change the net in Properties if needed.`);
        return gnd;
      }
    } catch {
      // Fall through: a pour without a net is still valid (shielding).
    }
    useEditor.getState().log("warning", "The new pour has no net — pick one in Properties so it connects.");
    return "";
  };
  return {
    id,
    hint: () =>
      pts.length
        ? `Click to add corners of the ${label} · click the first corner, double-click or right-click to close · Shift: 45° · Backspace removes a corner`
        : `Click to start the ${label}`,
    reset() {
      pts = [];
    },
    move(_env, e) {
      shift = e.shift;
    },
    down(env, e) {
      shift = e.shift;
      const { p, closes } = next(env);
      if (closes) return close(env);
      if (pts.length && samePoint(p, pts[pts.length - 1])) return;
      pts.push(p);
    },
    dblclick(env) {
      close(env);
    },
    key(_env, key) {
      if (key === "Backspace" && pts.length) {
        pts.pop();
        return true;
      }
      return false;
    },
    cancel(env) {
      if (!pts.length) return false;
      close(env);
      return true;
    },
    preview: (env) => {
      if (!pts.length) return [];
      const { p, closes } = next(env);
      const it = build(env, closes ? pts : [...pts, p]);
      return it ? [it] : [];
    },
    overlay: (env) => {
      if (!pts.length) return [];
      const px = 1 / env.scale();
      const { p, closes } = next(env);
      return [...ringLines([...pts, p], px, [1, 0.82, 0.25, 1], false, 1.5), ...ringHandles(closes ? [pts[0]] : pts, px, false)];
    },
  };
}

// ---------- footprint placement ----------

export function footprintTool(): Tool {
  const build = (env: ToolEnv, p: Point): Item | null => {
    const pf = useEditor.getState().placement.footprint;
    if (!pf) return null;
    const pl = env.placement();
    return {
      kind: "footprint",
      designator: pf.designator,
      value: pf.value,
      footprint: pf.key,
      pos: p,
      rotation: pl.rotation,
      side: pl.mirror ? "bottom" : "top",
      locked: false,
      padNets: {},
      component: pf.component,
      parameters: {},
    };
  };
  return {
    id: "footprint",
    hint: () => "Click to place the footprint · Space rotate · L flips to the other side · Esc to finish",
    preview: (env) => {
      const it = build(env, env.cursor());
      return it ? [it] : [];
    },
    down(env) {
      const it = build(env, env.cursor());
      if (!it) return;
      void env.exec({ op: "add", items: [it] }).then((ok) => {
        const pf = useEditor.getState().placement.footprint;
        if (ok && pf) env.setPlacement({ footprint: { ...pf, designator: bumpDesignator(pf.designator) } });
      });
    },
    key(env, key) {
      if (key === " ") {
        env.setPlacement({ rotation: (env.placement().rotation + 90) % 360 });
        return true;
      }
      if (key === "l" || key === "L" || key === "x" || key === "X") {
        env.setPlacement({ mirror: !env.placement().mirror });
        return true;
      }
      return false;
    },
    cancel: () => false,
  };
}

function bumpDesignator(d: string): string {
  const m = /^(.*?)(\d+)$/.exec(d);
  return m ? `${m[1]}${Number(m[2]) + 1}` : d;
}

// Re-exported for the Properties panel's net pickers.
export type { BoardItem };
