/**
 * Minimal dependency-free vector PDF 1.4 writer for schematic and board-layer
 * prints. Drawings come in nanometers (Y up) and are mapped onto the page with
 * `paper_mm = (p - originNm) / 1e6 * scale`; the page origin is its
 * bottom-left corner, as in PDF.
 */

export type PdfRgba = [number, number, number, number];

export interface PdfLine {
  a: { x: number; y: number };
  b: { x: number; y: number };
  /** Stroke width in nm (0 = thinnest line the device can draw). */
  width: number;
  color: PdfRgba;
}

export interface PdfFill {
  /** Flat triangle list [x,y, x,y, x,y, …] in nm. */
  tris: number[];
  color: PdfRgba;
}

export interface PdfPage {
  widthMm: number;
  heightMm: number;
  /** Design point (nm) that lands on the page's bottom-left corner. */
  originNm: { x: number; y: number };
  /** Millimeters on paper per millimeter in the design. */
  scale: number;
  lines: PdfLine[];
  fills: PdfFill[];
  /** Small footer text (page number is added automatically). */
  title?: string;
}

export type PaperSize = "A4" | "A3" | "Letter";

const PT_PER_MM = 72 / 25.4;
const NM_PER_MM = 1e6;

const PAPER_MM: Record<PaperSize, [number, number]> = {
  A4: [210, 297],
  A3: [297, 420],
  Letter: [215.9, 279.4],
};

/** Page size, scale and origin that fit `boundsNm` centered on the paper. */
export function fitPage(
  boundsNm: { minX: number; minY: number; maxX: number; maxY: number },
  paper: PaperSize,
  landscape: boolean,
  marginMm = 10,
): Pick<PdfPage, "widthMm" | "heightMm" | "originNm" | "scale"> {
  const [w, h] = PAPER_MM[paper] ?? PAPER_MM.A4;
  const widthMm = landscape ? Math.max(w, h) : Math.min(w, h);
  const heightMm = landscape ? Math.min(w, h) : Math.max(w, h);
  const margin = Math.max(0, Math.min(marginMm, Math.min(widthMm, heightMm) / 2 - 1));
  const bw = (boundsNm.maxX - boundsNm.minX) / NM_PER_MM;
  const bh = (boundsNm.maxY - boundsNm.minY) / NM_PER_MM;
  const availW = widthMm - 2 * margin;
  const availH = heightMm - 2 * margin;
  let scale = 1;
  if (bw > 0 && bh > 0) scale = Math.min(availW / bw, availH / bh);
  else if (bw > 0) scale = availW / bw;
  else if (bh > 0) scale = availH / bh;
  if (!Number.isFinite(scale) || scale <= 0) scale = 1;
  const cx = (boundsNm.minX + boundsNm.maxX) / 2;
  const cy = (boundsNm.minY + boundsNm.maxY) / 2;
  const originNm = {
    x: (Number.isFinite(cx) ? cx : 0) - ((widthMm / 2) / scale) * NM_PER_MM,
    y: (Number.isFinite(cy) ? cy : 0) - ((heightMm / 2) / scale) * NM_PER_MM,
  };
  return { widthMm, heightMm, originNm, scale };
}

// ---------------------------------------------------------------------------
// Formatting helpers
// ---------------------------------------------------------------------------

/** Compact PDF real number (≤ 3 decimals, no exponent). */
function n(v: number): string {
  if (!Number.isFinite(v)) return "0";
  const r = Math.round(v * 1000) / 1000;
  if (Object.is(r, -0) || r === 0) return "0";
  let s = r.toFixed(3);
  s = s.replace(/0+$/, "").replace(/\.$/, "");
  return s;
}

function clamp01(v: number): number {
  return Number.isFinite(v) ? Math.min(1, Math.max(0, v)) : 0;
}

function rgb(c: PdfRgba): string {
  return `${n(clamp01(c[0]))} ${n(clamp01(c[1]))} ${n(clamp01(c[2]))}`;
}

function alphaOf(c: PdfRgba): number {
  const a = c[3];
  return a === undefined ? 1 : Math.round(clamp01(a) * 100) / 100;
}

/** WinAnsi literal string: escapes delimiters, octal-escapes non-ASCII. */
function pdfString(s: string): string {
  let out = "(";
  for (const ch of s) {
    let code = ch.codePointAt(0) ?? 63;
    if (code > 255) code = 63; // '?'
    if (ch === "(" || ch === ")" || ch === "\\") out += "\\" + ch;
    else if (code < 32 || code > 126) out += "\\" + code.toString(8).padStart(3, "0");
    else out += ch;
  }
  return out + ")";
}

/** Info-dictionary text string: ASCII literal, or UTF-16BE hex with BOM. */
function infoString(s: string): string {
  if (/^[\x20-\x7e]*$/.test(s)) return pdfString(s);
  let hex = "<FEFF";
  for (let i = 0; i < s.length; i++) hex += s.charCodeAt(i).toString(16).padStart(4, "0").toUpperCase();
  return hex + ">";
}

function pdfDate(d: Date): string {
  const p = (v: number) => String(v).padStart(2, "0");
  return `(D:${d.getUTCFullYear()}${p(d.getUTCMonth() + 1)}${p(d.getUTCDate())}${p(d.getUTCHours())}${p(d.getUTCMinutes())}${p(d.getUTCSeconds())}Z)`;
}

/** Helvetica advance widths (1/1000 em) for ASCII 32–126. */
const HELVETICA = [
  278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278, 278, 556, 556, 556, 556, 556, 556, 556, 556, 556, 556, 278, 278, 584,
  584, 584, 556, 1015, 667, 667, 722, 722, 667, 611, 778, 722, 278, 500, 667, 556, 833, 722, 778, 667, 778, 722, 667, 611, 722, 667, 944, 667,
  667, 611, 278, 278, 278, 469, 556, 333, 556, 556, 500, 556, 556, 278, 556, 556, 222, 222, 500, 222, 833, 556, 556, 556, 556, 333, 500, 278,
  556, 500, 722, 500, 500, 500, 334, 260, 334, 584,
];

function textWidthPt(s: string, size: number): number {
  let w = 0;
  for (const ch of s) {
    const c = ch.codePointAt(0) ?? 63;
    w += c >= 32 && c <= 126 ? HELVETICA[c - 32] : 556;
  }
  return (w / 1000) * size;
}

// ---------------------------------------------------------------------------
// Content streams
// ---------------------------------------------------------------------------

interface Gs {
  alphas: Map<number, string>;
}

function gsName(gs: Gs, alpha: number): string {
  let name = gs.alphas.get(alpha);
  if (!name) {
    name = `GS${gs.alphas.size + 1}`;
    gs.alphas.set(alpha, name);
  }
  return name;
}

function pageContent(page: PdfPage, index: number, count: number, gs: Gs): string {
  const k = (page.scale * PT_PER_MM) / NM_PER_MM; // points per nm
  const ox = page.originNm.x;
  const oy = page.originNm.y;
  const X = (x: number) => n((x - ox) * k);
  const Y = (y: number) => n((y - oy) * k);
  const out: string[] = ["1 J 1 j"];
  let curAlpha = 1;
  const setAlpha = (a: number) => {
    if (a !== curAlpha) {
      out.push(`/${gsName(gs, a)} gs`);
      curAlpha = a;
    }
  };

  // Fills first (bodies, pours, pads), merged per run of equal color.
  let fi = 0;
  const fills = page.fills ?? [];
  while (fi < fills.length) {
    const color = fills[fi].color;
    const key = rgb(color) + "|" + alphaOf(color);
    const parts: string[] = [];
    while (fi < fills.length && rgb(fills[fi].color) + "|" + alphaOf(fills[fi].color) === key) {
      const t = fills[fi].tris ?? [];
      for (let i = 0; i + 5 < t.length; i += 6) {
        let [x1, y1, x2, y2, x3, y3] = [t[i], t[i + 1], t[i + 2], t[i + 3], t[i + 4], t[i + 5]];
        // Same orientation for every triangle so the nonzero rule unions them.
        if ((x2 - x1) * (y3 - y1) - (y2 - y1) * (x3 - x1) < 0) [x2, y2, x3, y3] = [x3, y3, x2, y2];
        parts.push(`${X(x1)} ${Y(y1)} m ${X(x2)} ${Y(y2)} l ${X(x3)} ${Y(y3)} l h`);
      }
      fi++;
    }
    if (parts.length) {
      setAlpha(alphaOf(color));
      out.push(`${rgb(color)} rg`);
      out.push(parts.join("\n"));
      out.push("f");
    }
  }

  // Lines, grouped by runs of equal color and width; chained when contiguous.
  let li = 0;
  const lines = page.lines ?? [];
  while (li < lines.length) {
    const first = lines[li];
    const key = rgb(first.color) + "|" + alphaOf(first.color) + "|" + first.width;
    const parts: string[] = [];
    let last: { x: number; y: number } | null = null;
    while (li < lines.length) {
      const l = lines[li];
      if (rgb(l.color) + "|" + alphaOf(l.color) + "|" + l.width !== key) break;
      if (!last || last.x !== l.a.x || last.y !== l.a.y) parts.push(`${X(l.a.x)} ${Y(l.a.y)} m`);
      parts.push(`${X(l.b.x)} ${Y(l.b.y)} l`);
      last = l.b;
      li++;
    }
    setAlpha(alphaOf(first.color));
    out.push(`${rgb(first.color)} RG ${n(Math.max(0, first.width) * k)} w`);
    out.push(parts.join("\n"));
    out.push("S");
  }

  // Footer: title left, page number right (8 pt Helvetica, 5 mm from the bottom).
  setAlpha(1);
  const size = 8;
  const marginPt = 8 * PT_PER_MM;
  const baseline = 5 * PT_PER_MM;
  const pageW = page.widthMm * PT_PER_MM;
  const label = count > 1 ? `${index + 1} / ${count}` : "";
  out.push("0 0 0 rg");
  if (page.title) out.push(`BT /F1 ${size} Tf ${n(marginPt)} ${n(baseline)} Td ${pdfString(page.title)} Tj ET`);
  if (label) out.push(`BT /F1 ${size} Tf ${n(pageW - marginPt - textWidthPt(label, size))} ${n(baseline)} Td ${pdfString(label)} Tj ET`);
  return out.join("\n") + "\n";
}

// ---------------------------------------------------------------------------
// File assembly
// ---------------------------------------------------------------------------

/** Builds a PDF from pages; returns the file bytes. */
export function buildPdf(pages: PdfPage[], meta?: { title?: string; author?: string }): Uint8Array {
  const enc = new TextEncoder();
  const list = pages.length ? pages : [{ widthMm: 210, heightMm: 297, originNm: { x: 0, y: 0 }, scale: 1, lines: [], fills: [] } as PdfPage];
  const gs: Gs = { alphas: new Map() };

  // Object numbers: 1 catalog, 2 pages, 3 font, 4 ext-gstate dict, 5 info,
  // then (page, content) pairs.
  const contents = list.map((p, i) => pageContent(p, i, list.length, gs));
  const objs: string[] = [];
  const pageIds = list.map((_, i) => 6 + 2 * i);
  objs[1] = "<< /Type /Catalog /Pages 2 0 R >>";
  objs[2] = `<< /Type /Pages /Kids [${pageIds.map((id) => `${id} 0 R`).join(" ")}] /Count ${list.length} >>`;
  objs[3] = "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>";
  objs[4] = `<< ${[...gs.alphas].map(([a, name]) => `/${name} << /Type /ExtGState /CA ${n(a)} /ca ${n(a)} >>`).join(" ")} >>`;
  const info = [`/Producer (PCBPlannerCAD)`, `/CreationDate ${pdfDate(new Date())}`];
  if (meta?.title) info.push(`/Title ${infoString(meta.title)}`);
  if (meta?.author) info.push(`/Author ${infoString(meta.author)}`);
  objs[5] = `<< ${info.join(" ")} >>`;
  list.forEach((p, i) => {
    const w = n(Math.max(1, p.widthMm) * PT_PER_MM);
    const h = n(Math.max(1, p.heightMm) * PT_PER_MM);
    objs[pageIds[i]] =
      `<< /Type /Page /Parent 2 0 R /MediaBox [0 0 ${w} ${h}] /Resources << /Font << /F1 3 0 R >> /ExtGState 4 0 R >> /Contents ${pageIds[i] + 1} 0 R >>`;
    const body = enc.encode(contents[i]);
    objs[pageIds[i] + 1] = `<< /Length ${body.length} >>\nstream\n${contents[i]}endstream`;
  });

  const chunks: Uint8Array[] = [];
  let offset = 0;
  const push = (b: Uint8Array) => {
    chunks.push(b);
    offset += b.length;
  };
  push(enc.encode("%PDF-1.4\n"));
  push(new Uint8Array([0x25, 0xe2, 0xe3, 0xcf, 0xd3, 0x0a])); // binary marker comment
  const offsets: number[] = [];
  for (let id = 1; id < objs.length; id++) {
    offsets[id] = offset;
    push(enc.encode(`${id} 0 obj\n${objs[id]}\nendobj\n`));
  }
  const xref = offset;
  let table = `xref\n0 ${objs.length}\n0000000000 65535 f \n`;
  for (let id = 1; id < objs.length; id++) table += `${String(offsets[id]).padStart(10, "0")} 00000 n \n`;
  table += `trailer\n<< /Size ${objs.length} /Root 1 0 R /Info 5 0 R >>\nstartxref\n${xref}\n%%EOF\n`;
  push(enc.encode(table));

  const out = new Uint8Array(offset);
  let at = 0;
  for (const c of chunks) {
    out.set(c, at);
    at += c.length;
  }
  return out;
}

/**
 * Builds a 2-page PDF (with non-ASCII text and translucent colors) and checks
 * that startxref points at the xref table, every xref offset points at
 * "N 0 obj", and stream lengths match.
 */
export function pdfSelfCheck(): boolean {
  const view = fitPage({ minX: 0, minY: 0, maxX: 100e6, maxY: 50e6 }, "A4", true);
  const page = (title: string): PdfPage => ({
    ...view,
    title,
    lines: [
      { a: { x: 0, y: 0 }, b: { x: 100e6, y: 0 }, width: 0.25e6, color: [1, 0, 0, 1] },
      { a: { x: 100e6, y: 0 }, b: { x: 100e6, y: 50e6 }, width: 0.25e6, color: [1, 0, 0, 1] },
      { a: { x: 0, y: 50e6 }, b: { x: 0, y: 0 }, width: 0, color: [0, 0, 1, 0.5] },
    ],
    fills: [
      { tris: [10e6, 10e6, 20e6, 10e6, 20e6, 20e6, 10e6, 10e6, 10e6, 20e6, 20e6, 20e6], color: [0, 0.6, 0, 1] },
      { tris: [30e6, 10e6, 40e6, 10e6, 35e6, 20e6], color: [0, 0.6, 0, 0.4] },
    ],
  });
  const bytes = buildPdf([page("Schematic – Ünïcode (test) \\ page"), page("Board")], { title: "Self check ✓", author: "PCBPlannerCAD" });
  let text = "";
  for (let i = 0; i < bytes.length; i++) text += String.fromCharCode(bytes[i]);
  if (!text.startsWith("%PDF-1.4\n") || !text.endsWith("%%EOF\n")) return false;
  const sx = /startxref\n(\d+)\n%%EOF\n$/.exec(text);
  if (!sx) return false;
  const xrefAt = Number(sx[1]);
  if (text.slice(xrefAt, xrefAt + 5) !== "xref\n") return false;
  const head = /^xref\n0 (\d+)\n/.exec(text.slice(xrefAt));
  if (!head) return false;
  const count = Number(head[1]);
  const rows = text.slice(xrefAt + head[0].length).split("\n");
  if (rows[0] !== "0000000000 65535 f ") return false;
  for (let id = 1; id < count; id++) {
    const row = rows[id];
    if (!row || row.length !== 19 || !row.endsWith(" 00000 n ")) return false;
    const off = Number(row.slice(0, 10));
    if (text.slice(off, off + `${id} 0 obj\n`.length) !== `${id} 0 obj\n`) return false;
  }
  // Stream lengths.
  const re = /<< \/Length (\d+) >>\nstream\n/g;
  let m: RegExpExecArray | null;
  let streams = 0;
  while ((m = re.exec(text))) {
    const start = m.index + m[0].length;
    if (text.slice(start + Number(m[1]), start + Number(m[1]) + 9) !== "endstream") return false;
    streams++;
  }
  return streams === 2 && text.includes("/Count 2") && text.includes("/CA 0.5");
}
