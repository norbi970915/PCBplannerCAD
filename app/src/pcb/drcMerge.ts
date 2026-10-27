// Online DRC bookkeeping: which objects a board change touched, and how a
// near-DRC of those objects updates the markers without a full re-check.

import type { DrcViolation, Item, ItemId } from "../core/types";

/** Batch-only checks; online DRC leaves them to the ratsnest and T D. */
export const ONLINE_SKIP = new Set(["unrouted"]);

export interface Touched {
  /** Present objects to re-check (pours whose only change is a refill are left out). */
  check: Set<ItemId>;
  /** Markers mentioning these objects are dropped before the fresh results come in. */
  drop: Set<ItemId>;
}

/** Same pour except its computed fill (the backend sends fresh objects, so compare by value). */
const fillOnly = (a: Item, b: Item) =>
  a.kind === "zone" && b.kind === "zone" && a.layer === b.layer && a.net === b.net && a.keepout === b.keepout && a.clearance === b.clearance &&
  a.outline.length === b.outline.length && a.outline.every((p, i) => p.x === b.outline[i].x && p.y === b.outline[i].y);

/** Objects that differ between two item maps. A refilled pour is not re-checked: its fill already obeys clearances. */
export function touchedBetween(prev: ReadonlyMap<ItemId, Item>, next: ReadonlyMap<ItemId, Item>, into: Touched = { check: new Set(), drop: new Set() }): Touched {
  for (const [id, item] of next) {
    const old = prev.get(id);
    if (old === item) continue;
    if (old && fillOnly(old, item)) continue;
    into.check.add(id);
    into.drop.add(id);
  }
  for (const id of prev.keys()) {
    if (!next.has(id)) {
      into.drop.add(id);
      into.check.delete(id);
    }
  }
  return into;
}

/** Current markers minus those about changed objects, plus the fresh near-DRC results (deduplicated). */
export function mergeViolations(current: readonly DrcViolation[], drop: ReadonlySet<ItemId>, fresh: readonly DrcViolation[]): DrcViolation[] {
  const kept = current.filter((v) => !ONLINE_SKIP.has(v.code) && !v.items.some((id) => drop.has(id)));
  const key = (v: DrcViolation) => `${v.code}|${[...v.items].sort((a, b) => a - b).join(",")}|${v.layer ?? ""}`;
  const seen = new Set(kept.map(key));
  for (const v of fresh) {
    if (ONLINE_SKIP.has(v.code) || seen.has(key(v))) continue;
    seen.add(key(v));
    kept.push(v);
  }
  return kept;
}
