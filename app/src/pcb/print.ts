// Builds a one-page vector PDF of a document from its drawn primitives.

import type { DocState } from "../core/store";
import { drawItem, type DrawContext } from "../editor/draw";
import { boardTheme, schematicTheme, type Theme } from "../editor/theme";
import { buildPdf, fitPage, type PaperSize, type PdfFill, type PdfLine, type PdfRgba } from "../output/pdf";

export interface PrintOptions {
  paper: PaperSize;
  landscape: boolean;
  /** Black on white instead of screen colors. */
  mono: boolean;
}

const BLACK: PdfRgba = [0, 0, 0, 1];

/** Board colors are tuned for a dark background; darken light ones for paper. */
function onPaper(c: PdfRgba): PdfRgba {
  const lum = 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
  const k = lum > 0.6 ? 0.6 / lum : 1;
  return [c[0] * k, c[1] * k, c[2] * k, 1];
}

export function buildPrint(doc: DocState, opts: PrintOptions): Uint8Array {
  const kind = doc.info.kind;
  const board = kind === "pcb" || kind === "footprint";
  const theme: Theme = board ? boardTheme(0, false) : schematicTheme("light");
  const ctx: DrawContext = { kind, cache: doc.cache, footprints: doc.footprints };

  const ranked: { rank: number; lines: PdfLine[]; fills: PdfFill[] }[] = [];
  const group = (rank: number) => {
    let g = ranked.find((x) => x.rank === rank);
    if (!g) ranked.push((g = { rank, lines: [], fills: [] }));
    return g;
  };
  const bounds = { minX: Infinity, minY: Infinity, maxX: -Infinity, maxY: -Infinity };
  const grow = (x: number, y: number) => {
    bounds.minX = Math.min(bounds.minX, x);
    bounds.minY = Math.min(bounds.minY, y);
    bounds.maxX = Math.max(bounds.maxX, x);
    bounds.maxY = Math.max(bounds.maxY, y);
  };

  for (const item of doc.items.values()) {
    if (item.kind === "board") continue;
    const prims = drawItem(item, ctx);
    for (const s of prims.segs) {
      if (s.role === "ratsnest" || s.role === "drc") continue;
      const c = theme.color(s.role);
      const color = opts.mono ? BLACK : board ? onPaper(c) : c;
      group(theme.rank(s.role)).lines.push({ a: s.a, b: s.b, width: s.w, color });
      grow(s.a.x, s.a.y);
      grow(s.b.x, s.b.y);
    }
    for (const f of prims.fills) {
      const c = theme.color(f.role);
      // Mono: symbol bodies and sheets stay white, everything else prints black.
      const paperFill = f.role === "bodyFill" || f.role === "sheetFill" || f.role === "graphicFill" || f.role === "hole";
      const color: PdfRgba = opts.mono ? (paperFill ? [1, 1, 1, 1] : BLACK) : board ? (f.role === "hole" ? [1, 1, 1, 1] : onPaper(c)) : c;
      group(theme.rank(f.role)).fills.push({ tris: f.tris, color });
      for (let i = 0; i < f.tris.length; i += 2) grow(f.tris[i], f.tris[i + 1]);
    }
  }
  if (!Number.isFinite(bounds.minX)) throw new Error("Nothing to print — the document is empty.");

  ranked.sort((a, b) => a.rank - b.rank);
  const page = fitPage(bounds, opts.paper, opts.landscape);
  return buildPdf(
    [{ ...page, lines: ranked.flatMap((g) => g.lines), fills: ranked.flatMap((g) => g.fills), title: doc.info.title }],
    { title: doc.info.title, author: "PCBPlannerCAD" },
  );
}
