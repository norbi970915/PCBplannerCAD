import { useEffect, useRef } from "react";
import type { CachedPart, DocKind, Item, LibSymbol } from "../core/types";
import { Camera } from "./camera";
import { useTheme } from "../core/theme";
import { Scene } from "./scene";
import { Renderer } from "./renderer";
import { themeFor } from "./theme";

/** Static, auto-fitted rendering of a few items (library previews, wizard). */
export function PreviewCanvas({
  items,
  kind,
  symbol,
  cache,
  height = 220,
}: {
  items: Item[];
  kind: DocKind;
  symbol?: LibSymbol | null;
  cache?: Record<string, CachedPart>;
  height?: number;
}) {
  const hostRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const stateRef = useRef<{ renderer: Renderer; cam: Camera; scene: Scene } | null>(null);
  const drawRef = useRef<() => void>(() => {});
  const ui = useTheme((s) => s.resolved);

  useEffect(() => {
    let renderer: Renderer;
    try {
      renderer = new Renderer(canvasRef.current!);
    } catch {
      return;
    }
    stateRef.current = { renderer, cam: new Camera(), scene: new Scene() };
    const ro = new ResizeObserver(() => drawRef.current());
    ro.observe(hostRef.current!);
    return () => {
      ro.disconnect();
      renderer.dispose();
      stateRef.current = null;
    };
  }, []);

  useEffect(() => {
    drawRef.current = () => {
      const s = stateRef.current;
      const host = hostRef.current;
      if (!s || !host) return;
      const r = host.getBoundingClientRect();
      if (r.width < 2 || r.height < 2) return;
      s.cam.width = r.width;
      s.cam.height = r.height;
      s.renderer.resize(r.width, r.height, window.devicePixelRatio || 1);
      const theme = themeFor(kind, 0, false, ui);
      s.scene.update(new Map(items.map((it, i) => [i, it])), { kind, cache: cache ?? {}, symbol });
      s.renderer.setScene(s.scene.build(theme, { selection: new Set(), hover: null }));
      s.renderer.setHighlights(new Map(), s.scene.styleCount);
      s.renderer.setOverlay([]);
      const b = s.scene.bounds();
      if (b) s.cam.fit(b, 16);
      s.renderer.render(s.cam, { step: 1e12, background: theme.background, ...theme.grid });
    };
    drawRef.current();
  }, [items, kind, symbol, cache, ui]);

  return (
    <div className="preview-canvas" ref={hostRef} style={{ height }}>
      <canvas ref={canvasRef} />
    </div>
  );
}
