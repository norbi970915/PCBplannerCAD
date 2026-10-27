import type { LayerId } from "../core/types";

export type ViaKind = "through" | "blind" | "buried" | "micro";

/** Laser-drilled microvias span one dielectric with a drill up to this size (mirrors the core). */
export const MICROVIA_MAX_DRILL = 150_000;

/** Copper layers a via from `a` to `b` connects, in stack order (inclusive). */
export function viaSpan(copper: LayerId[], a: LayerId, b: LayerId): LayerId[] {
  const i = copper.indexOf(a), j = copper.indexOf(b);
  if (i < 0 || j < 0) return [];
  return copper.slice(Math.min(i, j), Math.max(i, j) + 1);
}

/** Through, blind, buried or micro, from the copper span and drill (mirrors `Stackup::via_kind`). */
export function viaKind(copper: LayerId[], a: LayerId, b: LayerId, drill: number): ViaKind {
  const span = viaSpan(copper, a, b);
  const outer = (l: LayerId) => l === copper[0] || l === copper[copper.length - 1];
  if (span.length === copper.length) return "through";
  if (span.length === 2 && drill <= MICROVIA_MAX_DRILL) return "micro";
  return outer(a) || outer(b) ? "blind" : "buried";
}

export const VIA_KIND_LABEL: Record<ViaKind, string> = { through: "Through", blind: "Blind", buried: "Buried", micro: "Microvia" };
