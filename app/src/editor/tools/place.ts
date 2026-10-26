// Placement tools. Each builds items from the snapped cursor and the
// placement parameters (edited live in the Properties panel).

import { L } from "../../core/layers";
import { useEditor } from "../../core/store";
import type { Field, Item, ItemOf, Point } from "../../core/types";
import { NM_PER_MM } from "../../core/units";
import { connectionPoints } from "../draw";
import { onSegment, rotatePoint, samePoint } from "../shapes";
import { boardSettings, classOf, copperLayers, footprintTool, polygonTool, routeTool } from "./route";
import { measureTool } from "./measure";
import { reshapeTool } from "./polygonGesture";
import { translate } from "./select";
import type { Tool, ToolEnv } from "./types";

const mm = (v: number) => Math.round(v * NM_PER_MM);
const norm = (a: number) => ((a % 360) + 360) % 360;

/** "R12" → "R13", "D7" → "D8", "PIN" → "PIN". */
export function increment(s: string): string {
  const m = /^(.*?)(\d+)(\D*)$/.exec(s);
  if (!m) return s;
  const n = String(Number(m[2]) + 1).padStart(m[2].length, "0");
  return `${m[1]}${n}${m[3]}`;
}

const isSch = (env: ToolEnv) => env.doc().info.kind === "schematic" || env.doc().info.kind === "symbol";

function rotateKey(env: ToolEnv, key: string): boolean {
  if (key === " ") {
    env.setPlacement({ rotation: norm(env.placement().rotation + 90) });
    return true;
  }
  if (key === "x" || key === "X" || key === "y" || key === "Y") {
    env.setPlacement({ mirror: !env.placement().mirror });
    return true;
  }
  return false;
}

// ---------- single-click tools ----------

function oneClick(
  id: string,
  hint: string,
  build: (env: ToolEnv, p: Point) => Item[] | null,
  after?: (env: ToolEnv) => void,
): Tool {
  return {
    id,
    hint: () => `${hint} · Space rotate · Esc to finish`,
    preview: (env) => build(env, env.cursor()) ?? [],
    down(env) {
      const items = build(env, env.cursor());
      if (!items?.length) return;
      void env.exec({ op: "add", items }).then((ok) => ok && after?.(env));
    },
    key: rotateKey,
    cancel: () => false,
  };
}

function textItem(env: ToolEnv, p: Point): ItemOf<"text"> {
  const pl = env.placement();
  return { kind: "text", pos: p, text: pl.text || "Text", height: pl.textHeight, rotation: pl.rotation, hAlign: "left", vAlign: "bottom", layer: env.layer(), mirrored: false };
}

function pinItem(env: ToolEnv, p: Point): ItemOf<"pin"> {
  const pl = env.placement();
  return {
    kind: "pin",
    pos: p,
    length: pl.pinLength,
    rotation: pl.rotation,
    name: pl.pinName,
    number: pl.pinNumber,
    electrical: pl.pinType,
    shape: pl.pinShape,
    hidden: false,
  };
}

function padItem(env: ToolEnv, p: Point): ItemOf<"pad"> {
  const pl = env.placement();
  const hole = pl.padKind !== "smd";
  const active = env.doc().activeLayer;
  const smdLayer = active === L.bottomCopper ? L.bottomCopper : L.topCopper;
  return {
    kind: "pad",
    number: pl.padKind === "npth" ? "" : pl.padNumber,
    pos: p,
    rotation: pl.rotation,
    shape: pl.padShape,
    width: pl.padWidth,
    height: pl.padShape === "circle" ? pl.padWidth : pl.padHeight,
    cornerRatio: pl.padShape === "roundRect" ? pl.padCornerRatio : 0,
    padKind: pl.padKind,
    layer: hole ? L.multi : smdLayer,
    drill: hole ? { width: pl.padDrill, height: pl.padDrill } : null,
  };
}

function labelItem(env: ToolEnv, p: Point, scope: ItemOf<"netLabel">["scope"]): ItemOf<"netLabel"> {
  const pl = env.placement();
  return { kind: "netLabel", pos: p, text: pl.labelText || "NET", rotation: pl.rotation, scope, shape: pl.labelShape };
}

function powerItem(env: ToolEnv, p: Point): ItemOf<"powerPort"> {
  const pl = env.placement();
  return { kind: "powerPort", pos: p, net: pl.powerNet || "VCC", rotation: pl.rotation, style: pl.powerStyle };
}

/** Builds a Part from the picked component and the symbol's field templates. */
function partItem(env: ToolEnv, p: Point): ItemOf<"part"> | null {
  const pl = env.placement();
  const pick = pl.part;
  const cached = pick ? env.doc().cache[pick.key] : undefined;
  if (!pick || !cached) return null;
  const sym = cached.symbol;
  const value = cached.component.parameters.find((x) => x.name.toLowerCase() === "value")?.value || cached.component.name;
  const values: Record<string, string> = {
    Designator: pick.designator ?? `${sym.prefix || "U"}?`,
    Value: value,
    Footprint: cached.component.footprints[0] ?? "",
  };
  const templates = new Map(sym.fields.map((f) => [f.name, f]));
  const fields: Field[] = Object.entries(values).map(([name, v], i) => {
    const t = templates.get(name);
    const fallbackY = name === "Designator" ? mm(3.81) : name === "Value" ? mm(-3.81) : mm(-5.08);
    return {
      name,
      value: v,
      offset: t?.offset ?? { x: mm(2.54), y: fallbackY },
      rotation: t?.rotation ?? 0,
      height: t?.height ?? mm(1.27),
      hAlign: t?.hAlign ?? "left",
      vAlign: t?.vAlign ?? "middle",
      visible: t ? t.visible : i < 2,
    };
  });
  const base: ItemOf<"part"> = { kind: "part", pos: { x: 0, y: 0 }, rotation: 0, mirror: false, libRef: pick.key, unit: pick.unit, fields };
  // Apply mirror/rotation through the same rules as the backend (fields follow).
  let item: ItemOf<"part"> = base;
  if (pl.mirror) item = mirrorPart(item);
  if (pl.rotation) item = rotatePart(item, pl.rotation);
  return { ...item, pos: p };
}

function rotatePart(p: ItemOf<"part">, deg: number): ItemOf<"part"> {
  const o = { x: 0, y: 0 };
  return {
    ...p,
    rotation: norm(p.rotation + deg),
    fields: p.fields.map((f) => ({ ...f, offset: rotatePoint(f.offset, o, deg), rotation: norm(f.rotation + deg) })),
  };
}

function mirrorPart(p: ItemOf<"part">): ItemOf<"part"> {
  const flip = (f: Field): Field["hAlign"] => {
    const d = norm(f.rotation);
    const horizontal = !(d >= 45 && d < 135) && !(d >= 225 && d < 315);
    return horizontal ? (f.hAlign === "left" ? "right" : f.hAlign === "right" ? "left" : "center") : f.hAlign;
  };
  return {
    ...p,
    rotation: norm(-p.rotation),
    mirror: !p.mirror,
    fields: p.fields.map((f) => ({ ...f, offset: { x: -f.offset.x, y: f.offset.y }, hAlign: flip(f) })),
  };
}

// ---------- chains (lines, wires, buses, polylines) ----------

type ChainKind = "line" | "wire" | "bus" | "polyline";

/** Corner points between `a` and `b` for the current routing mode. */
function route(a: Point, b: Point, mode: number): Point[] {
  if (a.x === b.x || a.y === b.y || mode === 2) return [];
  return mode === 0 ? [{ x: b.x, y: a.y }] : [{ x: a.x, y: b.y }];
}

function segmentItems(kind: ChainKind, pts: Point[], env: ToolEnv): Item[] {
  const out: Item[] = [];
  for (let i = 1; i < pts.length; i++) {
    const a = pts[i - 1];
    const b = pts[i];
    if (samePoint(a, b)) continue;
    if (kind === "wire") out.push({ kind: "wire", a, b });
    else if (kind === "bus") out.push({ kind: "bus", a, b });
    else out.push({ kind: "line", a, b, width: isSch(env) ? 0 : env.placement().lineWidth, layer: env.layer() });
  }
  return out;
}

/** Junctions needed where new wire ends meet existing wires (T) or ≥3 ends meet. */
export function junctionsFor(env: ToolEnv, wires: ItemOf<"wire">[]): Item[] {
  const items = [...env.doc().items.values()];
  const existingWires = items.filter((i): i is ItemOf<"wire"> => i.kind === "wire");
  const junctions = items.filter((i): i is ItemOf<"junction"> => i.kind === "junction");
  const ctx = env.ctx();
  const pins = items.filter((i) => i.kind === "part").flatMap((i) => connectionPoints(i, ctx));
  const out: Item[] = [];
  const ends = wires.flatMap((w) => [w.a, w.b]);
  for (const e of ends) {
    if (junctions.some((j) => samePoint(j.pos, e)) || out.some((j) => j.kind === "junction" && samePoint(j.pos, e))) continue;
    let count = 0;
    let tee = false;
    for (const w of existingWires) {
      if (samePoint(w.a, e) || samePoint(w.b, e)) count++;
      else if (onSegment(e, w.a, w.b)) tee = true;
    }
    for (const w of wires) if (samePoint(w.a, e) || samePoint(w.b, e)) count++;
    count += pins.filter((p) => samePoint(p, e)).length;
    if (tee || count >= 3) out.push({ kind: "junction", pos: e });
  }
  return out;
}

/** Is `p` an existing connection anchor (pin, wire, label…)? Used to end wires. */
function onConnection(env: ToolEnv, p: Point, start: Point): boolean {
  if (samePoint(p, start)) return false;
  const ctx = env.ctx();
  for (const item of env.doc().items.values()) {
    if (item.kind === "wire" && onSegment(p, item.a, item.b)) return true;
    if (connectionPoints(item, ctx).some((c) => samePoint(c, p))) return true;
  }
  return false;
}

function chainTool(id: string, kind: ChainKind): Tool {
  let pts: Point[] = [];
  let mode = 0;
  const routed = kind === "wire" || kind === "bus";

  const commitTo = (env: ToolEnv, c: Point) => {
    const last = pts[pts.length - 1];
    const path = [last, ...(routed ? route(last, c, mode) : []), c];
    if (kind === "polyline") {
      pts.push(...path.slice(1));
      return;
    }
    const items = segmentItems(kind, path, env);
    if (!items.length) return;
    if (kind === "wire") items.push(...junctionsFor(env, items as ItemOf<"wire">[]));
    void env.exec({ op: "add", items });
    pts = [c];
  };

  const finish = (env: ToolEnv) => {
    if (kind === "polyline" && pts.length >= 2) {
      void env.exec({
        op: "add",
        items: [{ kind: "polyline", points: pts, closed: false, width: isSch(env) ? 0 : env.placement().lineWidth, layer: env.layer(), fill: "none" }],
      });
    }
    pts = [];
  };

  return {
    id,
    hint: () =>
      pts.length
        ? `Click to place the next point${routed ? " · Space changes corner mode" : ""}${kind === "polyline" ? " · Backspace removes a point" : ""} · Right-click / Esc to finish`
        : `Click to start the ${kind} · Esc to exit`,
    reset() {
      pts = [];
    },
    down(env) {
      const c = env.cursor();
      if (!pts.length) {
        pts = [c];
        return;
      }
      const start = pts[0];
      commitTo(env, c);
      if (kind === "wire" && onConnection(env, c, start)) pts = [];
    },
    dblclick(env) {
      finish(env);
    },
    key(_env, key) {
      if (key === " " && routed) {
        mode = (mode + 1) % 3;
        return true;
      }
      if (key === "Backspace" && pts.length > 1) {
        pts.pop();
        return true;
      }
      return false;
    },
    cancel(env) {
      if (!pts.length) return false;
      finish(env);
      return true;
    },
    preview(env) {
      if (!pts.length) return [];
      const c = env.cursor();
      const last = pts[pts.length - 1];
      const path = [...(kind === "polyline" ? pts : [last]), ...(routed ? route(last, c, mode) : []), c];
      return segmentItems(kind === "polyline" ? "line" : kind, path, env);
    },
  };
}

// ---------- multi-click shapes ----------

function shapeTool(
  id: string,
  clicks: number,
  label: string,
  build: (env: ToolEnv, pts: Point[]) => Item | null,
  after?: (env: ToolEnv) => void,
): Tool {
  let pts: Point[] = [];
  return {
    id,
    hint: () => `${label} — click point ${pts.length + 1} of ${clicks} · Esc to cancel`,
    reset() {
      pts = [];
    },
    down(env) {
      pts.push(env.cursor());
      if (pts.length < clicks) return;
      const item = build(env, pts);
      pts = [];
      if (item) void env.exec({ op: "add", items: [item] }).then((ok) => ok && after?.(env));
    },
    key: () => false,
    cancel() {
      if (!pts.length) return false;
      pts = [];
      return true;
    },
    preview(env) {
      if (!pts.length) return [];
      const item = build(env, [...pts, env.cursor()]);
      if (item) return [item];
      return pts.length === 1 ? [{ kind: "line", a: pts[0], b: env.cursor(), width: 0, layer: env.layer() }] : [];
    },
  };
}

const width = (env: ToolEnv) => (isSch(env) ? 0 : env.placement().lineWidth);

const rectTool = () =>
  shapeTool("rect", 2, "Rectangle", (env, [a, b]) =>
    b && a.x !== b.x && a.y !== b.y ? { kind: "rect", a, b, width: width(env), layer: env.layer(), fill: env.placement().fill } : null,
  );

const circleTool = () =>
  shapeTool("circle", 2, "Circle (center, then radius)", (env, [c, r]) => {
    const radius = r ? Math.round(Math.hypot(r.x - c.x, r.y - c.y)) : 0;
    return radius > 0 ? { kind: "circle", center: c, radius, width: width(env), layer: env.layer(), fill: env.placement().fill } : null;
  });

const arcTool = () =>
  shapeTool("arc", 3, "Arc (center, start, end — counter-clockwise)", (env, [c, s, e]) => {
    if (!s) return null;
    const radius = Math.round(Math.hypot(s.x - c.x, s.y - c.y));
    if (!radius) return null;
    const a0 = (Math.atan2(s.y - c.y, s.x - c.x) * 180) / Math.PI;
    const a1 = e ? (Math.atan2(e.y - c.y, e.x - c.x) * 180) / Math.PI : a0 + 90;
    const sweep = norm(a1 - a0) || 360;
    return { kind: "arc", center: c, radius, startAngle: norm(a0), sweep, width: width(env), layer: env.layer() };
  });

const sheetTool = () =>
  shapeTool(
    "sheet",
    2,
    "Sheet symbol (two corners)",
    (env, [a, b]) => {
      if (!b || a.x === b.x || a.y === b.y) return null;
      const name = env.placement().sheetName;
      return {
        kind: "sheet",
        pos: { x: Math.min(a.x, b.x), y: Math.max(a.y, b.y) },
        size: { x: Math.abs(b.x - a.x), y: Math.abs(b.y - a.y) },
        name,
        file: `${name.replace(/[^\w.-]+/g, "_")}.arsch`,
        entries: [],
      };
    },
    (env) => {
      const name = env.placement().sheetName;
      env.setPlacement({ sheetName: /\d$/.test(name) ? increment(name) : `${name}2` });
    },
  );

// ---------- registry ----------

export function makeTool(id: string): Tool | null {
  const st = () => useEditor.getState();
  switch (id) {
    case "line":
      return chainTool("line", "line");
    case "polyline":
      return chainTool("polyline", "polyline");
    case "wire":
      return chainTool("wire", "wire");
    case "bus":
      return chainTool("bus", "bus");
    case "rect":
      return rectTool();
    case "circle":
      return circleTool();
    case "arc":
      return arcTool();
    case "sheet":
      return sheetTool();
    case "text":
      return oneClick("text", "Click to place text", (env, p) => [textItem(env, p)]);
    case "pin":
      return oneClick(
        "pin",
        "Click to place a pin (the dot end connects)",
        (env, p) => [pinItem(env, p)],
        (env) => env.setPlacement({ pinNumber: increment(env.placement().pinNumber), pinName: increment(env.placement().pinName) }),
      );
    case "pad":
      return oneClick("pad", "Click to place a pad", (env, p) => [padItem(env, p)], (env) => env.setPlacement({ padNumber: increment(env.placement().padNumber) }));
    case "junction":
      return oneClick("junction", "Click to place a junction", (_env, p) => [{ kind: "junction", pos: p }]);
    case "noConnect":
      return oneClick("noConnect", "Click on a pin to mark it as intentionally unconnected", (_env, p) => [{ kind: "noConnect", pos: p }]);
    case "busEntry":
      return oneClick("busEntry", "Click to place a bus entry", (env, p) => [
        { kind: "busEntry", pos: p, size: rotatePoint({ x: mm(2.54), y: mm(2.54) }, { x: 0, y: 0 }, env.placement().rotation) },
      ]);
    case "label":
    case "globalLabel":
    case "port": {
      const scope = id === "label" ? "local" : id === "globalLabel" ? "global" : "hierarchical";
      return oneClick(id, `Click to place a ${scope} net label`, (env, p) => [labelItem(env, p, scope)], (env) =>
        env.setPlacement({ labelText: increment(env.placement().labelText) }),
      );
    }
    case "power":
      return oneClick("power", "Click to place a power port", (env, p) => [powerItem(env, p)]);
    case "route":
      return routeTool(false);
    case "diffPair":
      return routeTool(true);
    case "zone":
    case "keepout":
    case "outline":
      return polygonTool(id);
    case "reshape":
      return reshapeTool();
    case "polygonCut":
      return reshapeTool("cut");
    case "polygonSlice":
      return reshapeTool("slice");
    case "measure":
      return measureTool();
    case "footprint":
      return footprintTool();
    case "via":
      return oneClick("via", "Click to place a via", (env, p) => {
        const board = boardSettings(env);
        const cls = classOf(board, env.placement().net);
        const cu = copperLayers(board);
        return [
          {
            kind: "via",
            pos: p,
            diameter: cls?.viaDiameter ?? mm(0.6),
            drill: cls?.viaDrill ?? mm(0.3),
            fromLayer: cu[0],
            toLayer: cu[cu.length - 1],
            net: env.placement().net,
          },
        ];
      });
    case "paste":
      return {
        ...oneClick("paste", "Click to paste (repeat for more copies)", (env, p) => {
          const clip = env.placement().paste;
          if (!clip) return null;
          return clip.items.map((i) => translate(i, p.x - clip.anchor.x, p.y - clip.anchor.y));
        }),
        key: () => false,
      };
    case "part":
      return oneClick(
        "part",
        "Click to place the part · X mirror",
        (env, p) => {
          const item = partItem(env, p);
          return item ? [item] : null;
        },
        (env) => {
          const pick = env.placement().part;
          const cached = pick ? env.doc().cache[pick.key] : undefined;
          if (!pick || !cached) return;
          const units = cached.symbol.units.length;
          st().setPlacement({ part: { ...pick, unit: (pick.unit % units) + 1 } });
        },
      );
    default:
      return null;
  }
}
