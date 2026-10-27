import { useEffect, useRef, useState } from "react";
import { execute } from "../core/actions";
import { bus } from "../core/bus";
import { L } from "../core/layers";
import { useEditor } from "../core/store";
import { useTheme } from "../core/theme";
import { selectable, useSelectFilter } from "../core/selectFilter";
import { useContextMenu } from "../shell/ContextMenu";
import type { ItemId, LibSymbol, Point } from "../core/types";
import { NM_PER_MM } from "../core/units";
import { ActiveBar, PolygonEditBar, RoutingBar } from "./ActiveBar";
import { Camera } from "./camera";
import { InteractionFrame } from "./interactionFrame";
import { cursorPoint } from "../core/view";
import { connectionPoints, snapPoints, type DrawContext } from "./draw";
import { LayerTabs } from "./LayerTabs";
import { toolButton } from "./profiles";
import { Renderer, type DrawGroup, type DrawLine } from "./renderer";
import { buildPreview, Scene } from "./scene";
import { themeFor, type Theme } from "./theme";
import { makeTool } from "./tools/place";
import { selectTool } from "./tools/select";
import type { PointerInfo, ScreenRect, Tool, ToolEnv } from "./tools/types";

const CURSOR_ARM_PX = 10;
const CONNECT_SNAP_PX = 9;
const DRAG_PX = 4;

const st = () => useEditor.getState();

export function CanvasView({ docId, symbol }: { docId: string; symbol?: LibSymbol | null }) {
  const hostRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const symbolRef = useRef(symbol);
  const rebuildRef = useRef<() => void>(() => {});
  const [box, setBox] = useState<ScreenRect | null>(null);
  const [error, setError] = useState<string | null>(null);
  const kind = useEditor((s) => s.docs[docId]?.info.kind);

  useEffect(() => {
    symbolRef.current = symbol;
    rebuildRef.current();
  }, [symbol]);

  useEffect(() => {
    const host = hostRef.current!;
    const canvas = canvasRef.current!;
    let renderer: Renderer;
    try {
      renderer = new Renderer(canvas);
    } catch (e) {
      setError(String(e));
      return;
    }

    const cam = new Camera();
    const scene = new Scene();
    let tool: Tool = selectTool();
    let toolId = "select";
    let hover: ItemId | null = null;
    let cursorScreen: { x: number; y: number } | null = null;
    let cursor: Point = { x: 0, y: 0 };
    let raw: Point = { x: 0, y: 0 };
    let snapMark: Point | null = null;
    let pan: { x: number; y: number; sx: number; sy: number; moved: boolean; button: number } | null = null;
    let overlayDirty = true;
    let analysisDirty = true;
    let fitted = false;
    let theme: Theme = themeFor("schematic", 0, false);

    const doc = () => st().docs[docId];
    const ctx = (): DrawContext => ({ kind: doc().info.kind, cache: doc().cache, symbol: symbolRef.current, footprints: doc().footprints, pourFills: st().showPourFills, view: doc().view });
    const isActive = () => st().activeDocId === docId;

    const frames = new InteractionFrame<{ point: { x: number; y: number }; event: PointerInfo }>(
      ({ point, event }) => processPointerMove(point, event),
      () => {
        if (!doc()) return;
        if (analysisDirty) uploadAnalysis();
        if (overlayDirty) uploadOverlay();
        const grid = doc().view.grid;
        const hidden = [0, 0, 0, 0] as [number, number, number, number];
        renderer.render(cam, { step: doc().grid, background: theme.background,
          dot: grid ? theme.grid.dot : hidden, major: grid ? theme.grid.major : hidden, axis: grid ? theme.grid.axis : hidden });
      },
    );
    const requestRender = () => frames.invalidate();
    const rebuildHighlights = () => {
      const d = doc();
      if (!d) return;
      renderer.setHighlights(scene.highlights(theme, { selection: d.selection, marked: d.marked, hover, excluded: tool.excluded?.() }), scene.styleCount);
      requestRender();
    };

    const rebuildScene = () => {
      const d = doc();
      if (!d) return;
      theme = themeFor(d.info.kind, d.activeLayer, d.singleLayer, useTheme.getState().resolved);
      scene.update(d.items, ctx());
      renderer.setScene(scene.build(theme, { selection: new Set(), hover: null, excluded: tool.excluded?.() }));
      rebuildHighlights();
      analysisDirty = true;
      requestRender();
    };
    rebuildRef.current = () => {
      rebuildScene();
      rebuildOverlay();
    };

    const rebuildOverlay = () => { overlayDirty = true; requestRender(); };

    const uploadOverlay = () => {
      overlayDirty = false;
      const d = doc();
      if (!d) return;
      const groups: DrawGroup[] = buildPreview(tool.preview?.(env) ?? [], ctx(), theme, toolId === "select" ? 0.5 : 0.25);
      const lines: DrawLine[] = [...(tool.overlay?.(env) ?? [])];
      if (cursorScreen) {
        const arm = CURSOR_ARM_PX / cam.scale;
        const w = 1 / cam.scale;
        lines.push({ a: { x: cursor.x - arm, y: cursor.y }, b: { x: cursor.x + arm, y: cursor.y }, width: w, color: theme.cursor });
        lines.push({ a: { x: cursor.x, y: cursor.y - arm }, b: { x: cursor.x, y: cursor.y + arm }, width: w, color: theme.cursor });
        if (snapMark) {
          const r = 5 / cam.scale;
          const c = snapMark;
          const sq = [
            { x: c.x - r, y: c.y - r },
            { x: c.x + r, y: c.y - r },
            { x: c.x + r, y: c.y + r },
            { x: c.x - r, y: c.y + r },
          ];
          sq.forEach((p, i) => lines.push({ a: p, b: sq[(i + 1) % 4], width: 1.5 / cam.scale, color: [1, 0.35, 0.35, 1] }));
        }
      }
      groups.push({ rank: 1e6, lines, fills: tool.overlayFills?.(env) ?? [] });
      renderer.setOverlay(groups);
    };

    const uploadAnalysis = () => {
      analysisDirty = false;
      const d = doc();
      const lines: DrawLine[] = [];
      if (d.info.kind === "pcb") {
        const px = 1 / cam.scale;
        const rat = theme.color("ratsnest");
        if (d.view.airwires) for (const w of d.airwires) lines.push({ a: w.a, b: w.b, width: px, color: rat });
        const r = 6 * px;
        if (d.view.drc) for (const v of d.drc) {
          const c: [number, number, number, number] = v.severity === "error" ? [1, 0.25, 0.25, 1] : theme.color("drc");
          lines.push({ a: { x: v.pos.x - r, y: v.pos.y - r }, b: { x: v.pos.x + r, y: v.pos.y + r }, width: 2 * px, color: c });
          lines.push({ a: { x: v.pos.x - r, y: v.pos.y + r }, b: { x: v.pos.x + r, y: v.pos.y - r }, width: 2 * px, color: c });
        }
      }
      renderer.setAnalysis([{ rank: 1e6, lines, fills: [] }]);
    };

    const updateHint = () => {
      if (isActive()) {
        const hint = tool.hint(env);
        if (st().hint !== hint) st().setHint(hint);
      }
    };

    const ensureTool = () => {
      const d = doc();
      if (!d || d.tool === toolId) return;
      tool.reset?.();
      const impl = toolButton(d.info.kind, d.tool)?.tool ?? d.tool;
      tool = impl === "select" ? selectTool() : (makeTool(impl) ?? selectTool());
      toolId = d.tool;
      hover = null;
      updateHint();
      rebuildScene();
      rebuildOverlay();
    };

    /** Grid snap, overridden by a nearby connection point in schematics. */
    const computeCursor = (sx: number, sy: number) => {
      raw = cam.screenToWorld(sx, sy);
      const g = doc().grid;
      const useGrid = doc().view.snapToGrid && tool.gridSnap?.() !== false;
      cursor = cursorPoint(raw, g, useGrid);
      snapMark = null;
      const k = doc().info.kind;
      const idleSelection = toolId === "select" && tool.gridSnap?.() === false;
      if (!idleSelection && (k === "schematic" || (tool.snaps ? tool.snaps() : toolId !== "select"))) {
        const tol = CONNECT_SNAP_PX / cam.scale;
        let best = tol;
        const c = ctx();
        const board = k === "pcb" || k === "footprint";
        const skip = tool.excluded?.();
        for (const id of nearbyItems(raw, tol, board)) {
          const item = doc().items.get(id);
          if (!item || skip?.has(id)) continue;
          if (k !== "schematic" && !board && item.kind !== "pin" && item.kind !== "pad") continue;
          for (const p of board ? snapPoints(item, c) : connectionPoints(item, c)) {
            const d = Math.hypot(p.x - raw.x, p.y - raw.y);
            if (d < best && (p.x !== cursor.x || p.y !== cursor.y)) {
              best = d;
              snapMark = p;
            }
          }
        }
        if (snapMark) cursor = { x: Math.round(snapMark.x), y: Math.round(snapMark.y) };
      }
      const prev = st().cursor;
      if (isActive() && (!prev || prev.x !== cursor.x || prev.y !== cursor.y)) st().setCursor(cursor);
    };

    const nearbyItems = (p: Point, tol: number, board = false): ItemId[] =>
      scene.inRect({ minX: p.x - tol, minY: p.y - tol, maxX: p.x + tol, maxY: p.y + tol }, true, (i) => i.kind !== "text" && (board || i.kind !== "line"));

    const env: ToolEnv = {
      docId,
      doc,
      cursor: () => cursor,
      raw: () => raw,
      scale: () => cam.scale,
      scene,
      ctx,
      exec: (cmd) => execute(docId, cmd),
      placement: () => st().placement,
      setPlacement: (p) => st().setPlacement(p),
      select: (ids) => st().patchDoc(docId, { selection: new Set(ids) }),
      setTool: (id) => st().patchDoc(docId, { tool: id }),
      setBox,
      toWorld: (sx, sy) => cam.screenToWorld(sx, sy),
      redraw: (withScene) => {
        if (withScene) rebuildScene();
        rebuildOverlay();
        updateHint();
      },
      layer: () => {
        const d = doc();
        return d.info.kind === "schematic" || d.info.kind === "symbol" ? L.schematic : d.activeLayer;
      },
    };

    const local = (e: { clientX: number; clientY: number }) => {
      const r = canvas.getBoundingClientRect();
      return { x: e.clientX - r.left, y: e.clientY - r.top };
    };
    const info = (e: PointerEvent | MouseEvent): PointerInfo => {
      const p = local(e);
      return { sx: p.x, sy: p.y, shift: e.shiftKey, ctrl: e.ctrlKey, alt: e.altKey };
    };

    const updateHover = () => {
      const kind = doc().info.kind;
      const next = toolId === "select" ? scene.pick(raw, 5 / cam.scale, (i) => selectable(kind, i)) : null;
      if (next !== hover) {
        hover = next;
        rebuildHighlights();
      }
    };

    const exitOrCancel = () => {
      if (tool.cancel(env)) {
        if (toolId === "route" || toolId === "diffPair") st().patchDoc(docId, { tool: "select" });
        rebuildOverlay();
        updateHint();
        return;
      }
      if (toolId !== "select") {
        st().patchDoc(docId, { tool: "select" });
        st().setPlacement({ part: null });
      } else {
        st().patchDoc(docId, { selection: new Set(), marked: new Set() });
      }
    };

    const fit = () => {
      analysisDirty = true;
      const b = scene.bounds();
      if (b) cam.fit(b);
      else {
        cam.cx = 0;
        cam.cy = 0;
        const k = doc()?.info.kind;
        cam.scale = (k === "footprint" ? 40 : k === "pcb" ? 8 : 14) / NM_PER_MM;
      }
      rebuildOverlay();
    };

    const onPointerDown = (e: PointerEvent) => {
      frames.flush();
      canvas.setPointerCapture(e.pointerId);
      host.focus({ preventScroll: true });
      if (!isActive()) st().setActiveDoc(docId);
      const p = local(e);
      cursorScreen = p;
      computeCursor(p.x, p.y);
      if (e.button === 1 || e.button === 2) {
        pan = { x: p.x, y: p.y, sx: p.x, sy: p.y, moved: false, button: e.button };
        if (hover !== null) { hover = null; rebuildHighlights(); }
        return;
      }
      if (e.button !== 0) return;
      tool.down?.(env, info(e));
      rebuildOverlay();
      updateHint();
    };

    const processPointerMove = (p: { x: number; y: number }, event: PointerInfo) => {
      cursorScreen = p;
      computeCursor(p.x, p.y);
      if (!pan) { tool.move?.(env, event); updateHover(); }
      rebuildOverlay();
      updateHint();
    };

    const onPointerMove = (e: PointerEvent) => {
      const p = local(e);
      if (pan) {
        cam.panBy(p.x - pan.x, p.y - pan.y);
        pan.x = p.x;
        pan.y = p.y;
        pan.moved ||= Math.hypot(p.x - pan.sx, p.y - pan.sy) > DRAG_PX;
      }
      frames.queue({ point: p, event: { sx: p.x, sy: p.y, shift: e.shiftKey, ctrl: e.ctrlKey, alt: e.altKey } });
    };

    const onPointerUp = (e: PointerEvent) => {
      frames.flush();
      if (canvas.hasPointerCapture(e.pointerId)) canvas.releasePointerCapture(e.pointerId);
      if (pan) {
        const done = pan;
        pan = null;
        updateHover();
        if (!done.moved && done.button === 2) {
          if (toolId === "select" && !tool.cancel(env)) openContextMenu(e);
          else exitOrCancel();
        }
        return;
      }
      if (e.button === 0) {
        tool.up?.(env, info(e));
        const p = local(e);
        computeCursor(p.x, p.y);
        updateHover();
      }
      rebuildOverlay();
      updateHint();
    };

    /** Right-click in select mode: select what is under the pointer and offer actions. */
    const openContextMenu = (e: PointerEvent) => {
      const d = doc();
      const hit = scene.pick(raw, 5 / cam.scale, (i) => selectable(d.info.kind, i));
      if (hit !== null && !d.selection.has(hit)) st().patchDoc(docId, { selection: new Set([hit]) });
      useContextMenu.getState().open(e.clientX, e.clientY, docId);
    };

    const onDblClick = (e: MouseEvent) => {
      frames.flush();
      tool.dblclick?.(env, info(e));
      rebuildOverlay();
    };

    const onWheel = (e: WheelEvent) => {
      e.preventDefault();
      frames.flush();
      const p = local(e);
      cursorScreen = p;
      if (!e.shiftKey) analysisDirty = true;
      if (e.shiftKey) cam.panBy(-e.deltaY, 0);
      else cam.zoomAt(p.x, p.y, Math.pow(1.0015, -e.deltaY));
      computeCursor(p.x, p.y);
      rebuildOverlay();
    };

    const onPointerLeave = () => {
      if (pan) return;
      frames.flush();
      cursorScreen = null;
      snapMark = null;
      if (isActive()) st().setCursor(null);
      if (hover !== null) {
        hover = null;
        rebuildHighlights();
      }
      rebuildOverlay();
    };

    const onKeyDown = (e: KeyboardEvent) => {
      if (!isActive()) return;
      frames.flush();
      // The key menu (P, T, V…) or a context menu is open: keys belong to it.
      if (st().pendingKeys || useContextMenu.getState().at) return;
      const t = e.target as HTMLElement | null;
      if (t && (t.isContentEditable || ["INPUT", "TEXTAREA", "SELECT"].includes(t.tagName))) return;
      if (e.ctrlKey || e.altKey || e.metaKey) return;
      if (e.key === "Escape") {
        exitOrCancel();
        e.preventDefault();
        return;
      }
      if (doc().info.kind === "pcb" && toolId === "select" && e.key.toLowerCase() === "x") {
        st().patchDoc(docId, { tool: "route" });
        ensureTool();
        if (cursorScreen) computeCursor(cursorScreen.x, cursorScreen.y);
        tool.down?.(env, { sx: cursorScreen?.x ?? 0, sy: cursorScreen?.y ?? 0, ctrl: false, shift: e.shiftKey, alt: false });
        e.preventDefault(); e.stopImmediatePropagation();
        rebuildOverlay(); updateHint(); return;
      }
      if (tool.key?.(env, e.key, e)) {
        e.preventDefault();
        e.stopImmediatePropagation();
        rebuildOverlay();
        updateHint();
      }
    };

    const zoomBy = (factor: number) => {
      frames.flush();
      analysisDirty = true;
      const p = cursorScreen ?? { x: cam.width / 2, y: cam.height / 2 };
      cam.zoomAt(p.x, p.y, factor);
      if (cursorScreen) computeCursor(p.x, p.y);
      rebuildOverlay();
    };

    const resize = new ResizeObserver(() => {
      const r = host.getBoundingClientRect();
      if (r.width < 2 || r.height < 2) return;
      cam.width = r.width;
      cam.height = r.height;
      renderer.resize(r.width, r.height, window.devicePixelRatio || 1);
      analysisDirty = true;
      rebuildOverlay();
      if (!fitted) {
        fitted = true;
        fit();
      }
      requestRender();
    });
    resize.observe(host);

    const unsubscribe = useEditor.subscribe((s, prev) => {
      const d = s.docs[docId];
      const p = prev.docs[docId];
      if (!d) return;
      if (d.tool !== p?.tool) ensureTool();
      if (s.showPourFills !== prev.showPourFills) rebuildScene();
      if (
        d.items !== p?.items ||
        d.view !== p?.view ||
        d.activeLayer !== p?.activeLayer ||
        d.singleLayer !== p?.singleLayer ||
        d.cache !== p?.cache ||
        d.footprints !== p?.footprints
      ) {
        rebuildScene();
      }
      if (d.selection !== p?.selection || d.marked !== p?.marked) rebuildHighlights();
      if (d.view !== p?.view && cursorScreen) computeCursor(cursorScreen.x, cursorScreen.y);
      if (d.airwires !== p?.airwires || d.drc !== p?.drc) { analysisDirty = true; requestRender(); }
      if (d.epoch !== p?.epoch) {
        scene.update(d.items, ctx());
        fit();
      }
      if (d.focus && d.focus !== p?.focus) {
        cam.cx = d.focus.pos.x;
        cam.cy = d.focus.pos.y;
        cam.scale = Math.max(cam.scale, 12 / NM_PER_MM);
        analysisDirty = true;
        rebuildOverlay();
      }
      if (d.grid !== p?.grid) requestRender();
      if (s.placement !== prev.placement || d.items !== p?.items || d.selection !== p?.selection || d.view !== p?.view) rebuildOverlay();
      if (s.activeDocId !== prev.activeDocId && s.activeDocId === docId) updateHint();
    });

    const offTheme = useTheme.subscribe(() => {
      rebuildScene();
      rebuildOverlay();
    });

    const offFilter = useSelectFilter.subscribe((filter, previous) => {
      if (filter.on === previous.on || doc().info.kind !== "pcb") return;
      const d = doc();
      const selection = [...d.selection].filter(id => { const item = d.items.get(id); return item && selectable(d.info.kind, item); });
      if (selection.length !== d.selection.size) st().patchDoc(docId, { selection: new Set(selection) });
      updateHover();
      updateHint();
    });
    const offView = bus.on("view", ({ action }) => {
      if (!isActive()) return;
      if (action === "fit") fit();
      else if (action === "zoomIn") zoomBy(1.5);
      else if (action === "zoomOut") zoomBy(1 / 1.5);
      else if (action === "fitSelection") {
        analysisDirty = true;
        const b = scene.bounds(doc().selection);
        if (b) cam.fit(b, 80);
        rebuildOverlay();
      }
    });

    const onContext = (e: MouseEvent) => e.preventDefault();
    canvas.addEventListener("pointerdown", onPointerDown);
    canvas.addEventListener("pointermove", onPointerMove);
    canvas.addEventListener("pointerup", onPointerUp);
    canvas.addEventListener("pointerleave", onPointerLeave);
    canvas.addEventListener("dblclick", onDblClick);
    canvas.addEventListener("wheel", onWheel, { passive: false });
    canvas.addEventListener("contextmenu", onContext);
    window.addEventListener("keydown", onKeyDown, true);

    ensureTool();
    rebuildScene();
    rebuildOverlay();
    updateHint();

    return () => {
      frames.dispose();
      resize.disconnect();
      unsubscribe();
      offView();
      offFilter();
      offTheme();
      rebuildRef.current = () => {};
      canvas.removeEventListener("pointerdown", onPointerDown);
      canvas.removeEventListener("pointermove", onPointerMove);
      canvas.removeEventListener("pointerup", onPointerUp);
      canvas.removeEventListener("pointerleave", onPointerLeave);
      canvas.removeEventListener("dblclick", onDblClick);
      canvas.removeEventListener("wheel", onWheel);
      canvas.removeEventListener("contextmenu", onContext);
      window.removeEventListener("keydown", onKeyDown, true);
      renderer.dispose();
    };
  }, [docId]);

  const boardLike = kind === "pcb" || kind === "footprint";
  return (
    <div className="canvas-view">
      <div className="canvas-host" ref={hostRef} tabIndex={-1}>
        <canvas ref={canvasRef} className="canvas" />
        {box && <div className={`select-box ${box.touching ? "touching" : "inside"}`} style={{ left: box.x, top: box.y, width: box.w, height: box.h }} />}
        <ActiveBar docId={docId} />
        <PolygonEditBar docId={docId} />
        <RoutingBar docId={docId} />
        {error && <div className="canvas-error">{error}</div>}
      </div>
      {boardLike && <LayerTabs docId={docId} />}
    </div>
  );
}
