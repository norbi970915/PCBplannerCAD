import type { Coord } from "./types";

export type Units = "mm" | "mil";

export const NM_PER_MM = 1_000_000;
export const NM_PER_MIL = 25_400;

export const nmPerUnit = (u: Units) => (u === "mm" ? NM_PER_MM : NM_PER_MIL);

export const toUnits = (nm: Coord, u: Units) => nm / nmPerUnit(u);
export const fromUnits = (v: number, u: Units): Coord => Math.round(v * nmPerUnit(u));

export function formatLength(nm: Coord, u: Units): string {
  const v = toUnits(nm, u);
  return u === "mm" ? `${v.toFixed(3)}mm` : `${v.toFixed(1)}mil`;
}

/** Parses "1.5", "1.5mm" or "60mil"; bare numbers are read in `u`. */
export function parseLength(text: string, u: Units): Coord | null {
  const m = /^\s*(-?\d*\.?\d+)\s*(mm|mil)?\s*$/i.exec(text);
  if (!m) return null;
  return fromUnits(parseFloat(m[1]), (m[2]?.toLowerCase() as Units | undefined) ?? u);
}

/** Snap grid presets per unit system, in nanometers. */
export const GRID_PRESETS: Record<Units, Coord[]> = {
  mm: [0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1, 2.5, 5].map((v) => fromUnits(v, "mm")),
  mil: [1, 5, 10, 25, 50, 100, 250, 500].map((v) => fromUnits(v, "mil")),
};

export const DEFAULT_GRID: Record<Units, Coord> = {
  mm: fromUnits(0.1, "mm"),
  mil: fromUnits(5, "mil"),
};

/** Grid steps can be finer than ordinary dimension readouts. */
export const validGrid = (value:number) => Number.isSafeInteger(value) && value > 0 && value <= 1e9;
export function formatGrid(value:number,units:Units):string {
  return toUnits(value,units).toFixed(6).replace(/\.?0+$/,'')+units;
}
export function adjacentGrid(current:number,presets:readonly number[],step:1|-1):number {
  const sorted=[...new Set(presets)].filter(validGrid).sort((a,b)=>a-b);
  if(!sorted.length)return current;
  return step===1 ? sorted.find(v=>v>current)??current : sorted.reverse().find(v=>v<current)??current;
}
