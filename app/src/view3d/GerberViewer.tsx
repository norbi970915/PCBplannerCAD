import { useEffect, useMemo, useRef, useState } from "react";
import { open as openPicker } from "@tauri-apps/plugin-dialog";
import { FileArchive, FolderOpen, Maximize } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { cn } from "@/lib/utils";
import { api, type GerberFile } from "../core/api";
import { bus } from "../core/bus";
import { useTheme } from "../core/theme";

type Op = { fill: boolean; dark: boolean; width: number; path: Path2D };

interface Layer {
  file: GerberFile;
  color: string;
  ops: Op[];
  visible: boolean;
}

/** Display color and stacking order from the X2 file function or file name. */
function classify(f: GerberFile): { color: string; order: number } {
  const fn = (f.function ?? "").toLowerCase();
  const name = f.name.toLowerCase();
  const has = (...k: string[]) => k.some((s) => fn.includes(s) || name.includes(s));
  const bottom = fn.includes("bot") || /\.(gb\w|gbl|gbs|gbo|gbp)$/.test(name) || name.includes("b_") || name.includes("bottom");
  if (has("profile", "edge", ".gko", ".gm1")) return { color: "#e8d44d", order: 90 };
  if (has("plated", "drill", ".drl", ".xln", "nonplated")) return { color: "#d9d9d9", order: 95 };
  if (has("legend", "silk", ".gto", ".gbo")) return { color: bottom ? "#c9c3a8" : "#f4f4f4", order: bottom ? 15 : 80 };
  if (has("soldermask", "mask", ".gts", ".gbs")) return { color: bottom ? "#2e7a4a" : "#3c9a5c", order: bottom ? 12 : 70 };
  if (has("paste", ".gtp", ".gbp")) return { color: "#9aa1ab", order: bottom ? 11 : 75 };
  if (has("copper", ".gtl", ".gbl", ".g1", ".g2", ".g3", ".g4")) {
    if (bottom) return { color: "#4a86d6", order: 20 };
    if (fn.includes("inr") || /\.g\d$/.test(name)) return { color: "#c0a020", order: 40 };
    return { color: "#d8873a", order: 60 };
  }
  return { color: "#b48cff", order: 50 };
}

function buildOps(f: GerberFile): Op[] {
  const ops: Op[] = [];
  const op = (fill: boolean, dark: boolean, width: number) => {
    const last = ops[ops.length - 1];
    if (last && last.fill === fill && last.dark === dark && last.width === width) return last.path;
    const path = new Path2D();
    ops.push({ fill, dark, width, path });
    return path;
  };
  for (const s of f.shapes) {
    if ("stroke" in s) {
      const p = op(false, true, s.stroke.width);
      p.moveTo(s.stroke.a.x, s.stroke.a.y);
      p.lineTo(s.stroke.b.x, s.stroke.b.y);
    } else if ("arcStroke" in s) {
      const a = s.arcStroke;
      const p = op(false, true, a.width);
      const a0 = (a.startAngle * Math.PI) / 180;
      const a1 = ((a.startAngle + a.sweep) * Math.PI) / 180;
      p.moveTo(a.center.x + a.radius * Math.cos(a0), a.center.y + a.radius * Math.sin(a0));
      p.arc(a.center.x, a.center.y, a.radius, a0, a1, a.sweep < 0);
    } else if ("polygon" in s) {
      const pts = s.polygon.points;
      if (pts.length < 3) continue;
      const p = op(true, s.polygon.dark, 0);
      p.moveTo(pts[0].x, pts[0].y);
      for (let i = 1; i < pts.length; i++) p.lineTo(pts[i].x, pts[i].y);
      p.closePath();
    } else {
      const c = s.circle;
      const p = op(true, c.dark, 0);
      p.moveTo(c.center.x + c.radius, c.center.y);
      p.arc(c.center.x, c.center.y, c.radius, 0, Math.PI * 2);
    }
  }
  return ops;
}

export function GerberViewer({ initialPath }: { initialPath?: string }) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const [layers, setLayers] = useState<Layer[]>([]);
  const [source, setSource] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [active, setActive] = useState<number | null>(null);
  const cam = useRef({ x: 0, y: 0, k: 1e-5 });
  const [tick, setTick] = useState(0);
  const dark = useTheme((s) => s.resolved) === "dark";

  const load = async (path: string) => {
    setError(null);
    try {
      const files = await api.readGerbers(path);
      const list = files
        .map((file) => ({ file, ...classify(file) }))
        .sort((a, b) => a.order - b.order)
        .map(({ file, color }) => ({ file, color, ops: buildOps(file), visible: true }));
      setLayers(list);
      setSource(path);
      requestAnimationFrame(() => fitAll(list));
    } catch (e) {
      setError(String(e));
    }
  };

  useEffect(() => {
    // A single generated file stands for its folder; zips load as they are.
    const target = (p: string) => (/\.zip$/i.test(p) || !/\.[^\\/.]+$/.test(p) ? p : p.replace(/[\\/][^\\/]*$/, ""));
    if (initialPath) void load(target(initialPath));
    return bus.on("showGerbers", ({ path }) => path && void load(target(path)));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const bounds = useMemo(() => {
    let b: [number, number, number, number] | null = null;
    for (const l of layers) {
      const fb = l.file.bounds;
      if (!fb || !l.visible) continue;
      b = b ? [Math.min(b[0], fb[0].x), Math.min(b[1], fb[0].y), Math.max(b[2], fb[1].x), Math.max(b[3], fb[1].y)] : [fb[0].x, fb[0].y, fb[1].x, fb[1].y];
    }
    return b;
  }, [layers]);

  const fitAll = (list = layers) => {
    const c = canvasRef.current;
    let b: [number, number, number, number] | null = null;
    for (const l of list) {
      const fb = l.file.bounds;
      if (!fb) continue;
      b = b ? [Math.min(b[0], fb[0].x), Math.min(b[1], fb[0].y), Math.max(b[2], fb[1].x), Math.max(b[3], fb[1].y)] : [fb[0].x, fb[0].y, fb[1].x, fb[1].y];
    }
    if (!c || !b) return;
    const w = c.clientWidth;
    const h = c.clientHeight;
    const k = Math.min(w / Math.max(1, b[2] - b[0]), h / Math.max(1, b[3] - b[1])) * 0.9;
    cam.current = { x: (b[0] + b[2]) / 2, y: (b[1] + b[3]) / 2, k };
    setTick((t) => t + 1);
  };

  // Draw.
  useEffect(() => {
    const c = canvasRef.current;
    if (!c) return;
    const dpr = window.devicePixelRatio || 1;
    const w = c.clientWidth;
    const h = c.clientHeight;
    c.width = Math.max(1, Math.round(w * dpr));
    c.height = Math.max(1, Math.round(h * dpr));
    const g = c.getContext("2d")!;
    g.setTransform(1, 0, 0, 1, 0, 0);
    g.fillStyle = dark ? "#15171b" : "#20242a";
    g.fillRect(0, 0, c.width, c.height);
    const off = document.createElement("canvas");
    off.width = c.width;
    off.height = c.height;
    const o = off.getContext("2d")!;
    const { x, y, k } = cam.current;
    for (const [i, l] of layers.entries()) {
      if (!l.visible) continue;
      o.setTransform(1, 0, 0, 1, 0, 0);
      o.clearRect(0, 0, off.width, off.height);
      o.setTransform(k * dpr, 0, 0, -k * dpr, (w / 2 - x * k) * dpr, (h / 2 + y * k) * dpr);
      o.lineCap = "round";
      o.lineJoin = "round";
      for (const op of l.ops) {
        o.globalCompositeOperation = op.dark ? "source-over" : "destination-out";
        o.fillStyle = l.color;
        o.strokeStyle = l.color;
        if (op.fill) o.fill(op.path, "nonzero");
        else {
          o.lineWidth = Math.max(op.width, 1 / k);
          o.stroke(op.path);
        }
      }
      o.globalCompositeOperation = "source-over";
      g.globalAlpha = active === null || active === i ? 0.82 : 0.25;
      g.drawImage(off, 0, 0);
    }
    g.globalAlpha = 1;
  }, [layers, tick, dark, active]);

  // Pan / zoom.
  useEffect(() => {
    const c = canvasRef.current;
    if (!c) return;
    const wheel = (e: WheelEvent) => {
      e.preventDefault();
      const r = c.getBoundingClientRect();
      const { x, y, k } = cam.current;
      const mx = (e.clientX - r.left - r.width / 2) / k + x;
      const my = -(e.clientY - r.top - r.height / 2) / k + y;
      const nk = k * Math.pow(1.0015, -e.deltaY);
      cam.current = { k: nk, x: mx - (e.clientX - r.left - r.width / 2) / nk, y: my + (e.clientY - r.top - r.height / 2) / nk };
      setTick((t) => t + 1);
    };
    let drag: { x: number; y: number } | null = null;
    const down = (e: PointerEvent) => {
      drag = { x: e.clientX, y: e.clientY };
      c.setPointerCapture(e.pointerId);
    };
    const move = (e: PointerEvent) => {
      if (!drag) return;
      const { k } = cam.current;
      cam.current.x -= (e.clientX - drag.x) / k;
      cam.current.y += (e.clientY - drag.y) / k;
      drag = { x: e.clientX, y: e.clientY };
      setTick((t) => t + 1);
    };
    const up = () => (drag = null);
    const ro = new ResizeObserver(() => setTick((t) => t + 1));
    ro.observe(c);
    c.addEventListener("wheel", wheel, { passive: false });
    c.addEventListener("pointerdown", down);
    c.addEventListener("pointermove", move);
    c.addEventListener("pointerup", up);
    return () => {
      ro.disconnect();
      c.removeEventListener("wheel", wheel);
      c.removeEventListener("pointerdown", down);
      c.removeEventListener("pointermove", move);
      c.removeEventListener("pointerup", up);
    };
  }, []);

  const pick = async (directory: boolean) => {
    const p = await openPicker(directory ? { directory: true, title: "Folder with Gerber files" } : { filters: [{ name: "Zip", extensions: ["zip"] }] });
    if (typeof p === "string") void load(p);
  };

  const size = bounds ? `${((bounds[2] - bounds[0]) / 1e6).toFixed(2)} × ${((bounds[3] - bounds[1]) / 1e6).toFixed(2)} mm` : "";
  return (
    <div className="flex h-full bg-panel">
      <div className="flex w-56 shrink-0 flex-col border-r border-line">
        <div className="flex gap-1 border-b border-line bg-panel-head p-1.5">
          <Button variant="outline" size="sm" className="h-6 flex-1 text-[11px]" onClick={() => void pick(true)}>
            <FolderOpen /> Folder
          </Button>
          <Button variant="outline" size="sm" className="h-6 flex-1 text-[11px]" onClick={() => void pick(false)}>
            <FileArchive /> Zip
          </Button>
          <Button variant="ghost" size="icon" className="size-6" title="Fit" onClick={() => fitAll()}>
            <Maximize />
          </Button>
        </div>
        <div className="min-h-0 flex-1 overflow-auto py-1">
          {!layers.length && <p className="px-3 py-2 text-[11px] text-faint">{error ?? "Open a folder or .zip with Gerber and drill files."}</p>}
          {layers.map((l, i) => (
            <div
              key={l.file.name}
              onMouseEnter={() => setActive(i)}
              onMouseLeave={() => setActive(null)}
              className={cn("mx-1 flex items-center gap-2 rounded-[4px] px-1.5 py-1 hover:bg-hover", !l.visible && "opacity-50")}
              title={[l.file.function, ...l.file.warnings].filter(Boolean).join("\n")}
            >
              <Checkbox className="size-3.5" checked={l.visible} onCheckedChange={(v) => setLayers(layers.map((x, j) => (j === i ? { ...x, visible: v === true } : x)))} />
              <span className="size-3 shrink-0 rounded-[2px]" style={{ background: l.color }} />
              <span className="truncate text-xs">{l.file.name}</span>
              {l.file.warnings.length > 0 && <span className="ml-auto text-[10px] text-note-line">!</span>}
            </div>
          ))}
        </div>
        <div className="truncate border-t border-line px-2 py-1 font-mono text-[10px] text-faint" title={source ?? ""}>
          {size || source || ""}
        </div>
      </div>
      <canvas ref={canvasRef} className="min-w-0 flex-1 cursor-grab active:cursor-grabbing" style={{ width: "100%", height: "100%" }} />
    </div>
  );
}
