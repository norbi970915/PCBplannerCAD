// A single-stroke vector font so text renders
// through the same capsule pipeline as geometry and can later be emitted to
// Gerber as-is. Glyph units: cap height 6, x-height 4, descender -2.
// Format: "width:stroke|stroke", stroke = "x,y x,y …".

import type { HAlign, Point, VAlign } from "../core/types";
// Shared with the Rust core (crates/core/src/font.rs) so Gerber text matches the screen.
import glyphData from "./glyphs.json";

const RAW: Record<string, string> = glyphData;

const SPACING = 1.2;
const CAP = 6;
const LINE_PITCH = 10;
export const STROKE_RATIO = 0.13;

interface Glyph {
  width: number;
  strokes: number[][];
}

const glyphs = new Map<string, Glyph>();

function glyph(ch: string): Glyph {
  let g = glyphs.get(ch);
  if (g) return g;
  const raw = RAW[ch] ?? RAW[ch.normalize("NFD")[0]] ?? "4:0,0 4,0 4,6 0,6 0,0";
  const [w, body] = raw.split(":");
  g = {
    width: parseFloat(w),
    strokes: body
      ? body.split("|").map((s) => s.trim().split(/\s+/).flatMap((p) => p.split(",").map(Number)))
      : [],
  };
  glyphs.set(ch, g);
  return g;
}

interface Run {
  text: string;
  overbar: boolean;
}

/** Splits `~{overbar}` markup into runs. */
function runs(line: string): Run[] {
  const out: Run[] = [];
  const re = /~\{([^}]*)\}/g;
  let last = 0;
  for (let m = re.exec(line); m; m = re.exec(line)) {
    if (m.index > last) out.push({ text: line.slice(last, m.index), overbar: false });
    out.push({ text: m[1], overbar: true });
    last = m.index + m[0].length;
  }
  if (last < line.length) out.push({ text: line.slice(last), overbar: false });
  return out;
}

function lineWidth(line: string): number {
  let w = 0;
  for (const r of runs(line)) for (const ch of r.text) w += glyph(ch).width + SPACING;
  return Math.max(0, w - SPACING);
}

export interface TextStyle {
  height: number;
  rotation: number;
  hAlign: HAlign;
  vAlign: VAlign;
  mirrored?: boolean;
  /** Keep text readable (never upside down). Default true. */
  upright?: boolean;
}

/** Returns stroke segments in world nanometers. */
export function textSegments(text: string, pos: Point, style: TextStyle): [Point, Point][] {
  let rot = ((style.rotation % 360) + 360) % 360;
  let { hAlign, vAlign } = style;
  if (style.upright !== false && rot > 90.5 && rot <= 270.5) {
    rot -= 180;
    hAlign = hAlign === "left" ? "right" : hAlign === "right" ? "left" : "center";
    vAlign = vAlign === "bottom" ? "top" : vAlign === "top" ? "bottom" : "middle";
  }
  const unit = style.height / CAP;
  const lines = text.split("\n");
  const blockBottom = -(lines.length - 1) * LINE_PITCH;
  const dy = vAlign === "bottom" ? -blockBottom : vAlign === "top" ? -CAP : -(CAP + blockBottom) / 2;
  const rad = (rot * Math.PI) / 180;
  const cos = Math.cos(rad);
  const sin = Math.sin(rad);
  const mx = style.mirrored ? -1 : 1;
  const out: [Point, Point][] = [];
  const tf = (x: number, y: number): Point => {
    const lx = x * unit * mx;
    const ly = y * unit;
    return { x: pos.x + lx * cos - ly * sin, y: pos.y + lx * sin + ly * cos };
  };

  lines.forEach((line, li) => {
    const w = lineWidth(line);
    const dx = hAlign === "left" ? 0 : hAlign === "right" ? -w : -w / 2;
    const baseY = -li * LINE_PITCH + dy;
    let cx = dx;
    for (const run of runs(line)) {
      const startX = cx;
      for (const ch of run.text) {
        const g = glyph(ch);
        for (const s of g.strokes) {
          for (let i = 2; i + 1 < s.length; i += 2) {
            out.push([tf(cx + s[i - 2], baseY + s[i - 1]), tf(cx + s[i], baseY + s[i + 1])]);
          }
          if (s.length === 2) out.push([tf(cx + s[0], baseY + s[1]), tf(cx + s[0] + 0.01, baseY + s[1])]);
        }
        cx += g.width + SPACING;
      }
      if (run.overbar && cx > startX) out.push([tf(startX, baseY + CAP + 1), tf(cx - SPACING, baseY + CAP + 1)]);
    }
  });
  return out;
}

export function textStroke(height: number): number {
  return height * STROKE_RATIO;
}

/** Width of a single line of text at `height` (nanometers). */
export function textWidth(text: string, height: number): number {
  return (lineWidth(text) * height) / CAP;
}
