import { moveRejection } from "../editor/connectedMove";
import type { DocState } from "../core/store";
import type { IdItem } from "../core/types";
// Commands behind Edit ▸ Align / Distribute, U (select connection) and Unroute.

import { toast } from "sonner";
import { api } from "../core/api";
import { execute, report } from "../core/actions";
import { scheduleAnalysis } from "../core/pcbActions";
import { activeDoc, useEditor, violatorModeOf } from "../core/store";
import { bus } from "../core/bus";
import { NM_PER_MM } from "../core/units";
import { alignFootprints, distributeFootprints, packFootprints, unrouteFootprints, walkCopper, type AlignMode } from "./boardTools";

const st = () => useEditor.getState();

const moveVersions = new Map<string, number>();

/** One connected transaction for alignment, distribution and packing. */
async function moveFootprints(doc: DocState, items: IdItem[]): Promise<boolean> {
  const id = doc.info.id, token = (moveVersions.get(id) ?? 0) + 1;
  moveVersions.set(id, token);
  const placement = st().placement, routeMode = violatorModeOf(st());
  if (placement.moveMode === "disconnect") {
    const ok = await execute(id, { op: "replaceMany", items });
    return ok && moveVersions.get(id) === token && st().activeDocId === id;
  }
  const moves = items.flatMap(({ id, item }) => {
    const before = doc.items.get(id);
    return before?.kind === "footprint" && item.kind === "footprint"
      ? [{ id, delta: { x: item.pos.x - before.pos.x, y: item.pos.y - before.pos.y } }] : [];
  });
  try {
    const plan = await api.pcbDragEach(id, { moves, mode: placement.moveMode, routeMode, corner: placement.cornerStyle });
    if (moveVersions.get(id) !== token || st().activeDocId !== id) return false;
    const current = st().docs[id];
    if (!current || current.items !== doc.items || current.footprints !== doc.footprints || current.epoch !== doc.epoch || current.selection !== doc.selection) {
      st().setHint("Board changed while planning the move; try again."); return false;
    }
    if (st().placement.moveMode !== placement.moveMode || violatorModeOf(st()) !== routeMode || st().placement.cornerStyle !== placement.cornerStyle) {
      st().setHint("Move settings changed while planning; try again."); return false;
    }
    const rejection = moveRejection(plan, routeMode);
    if (rejection) { st().setHint(rejection); toast.warning(rejection); return false; }
    const ok = await execute(id, { op: "route", delete: plan.delete, add: plan.add, replace: plan.replace });
    if (ok && plan.blocked) toast.warning("Some connections were joined directly in Ignore mode. Run DRC to review them.");
    return ok && moveVersions.get(id) === token && st().activeDocId === id;
  } catch (error) { report(error); return false; }
}

const footprintsSelected = (min: number) => () => {
  const d = activeDoc();
  if (d?.info.kind !== "pcb") return false;
  let n = 0;
  for (const id of d.selection) if (d.items.get(id)?.kind === "footprint") n++;
  return n >= min;
};
export const canAlign = footprintsSelected(2);
export const canDistribute = footprintsSelected(3);
export const canUnroute = footprintsSelected(1);

export async function align(mode: AlignMode | "distributeX" | "distributeY") {
  const d = activeDoc();
  if (!d) return;
  const items = mode === "distributeX" || mode === "distributeY"
    ? distributeFootprints(d.items, d.footprints, d.selection, mode === "distributeX" ? "x" : "y")
    : alignFootprints(d.items, d.footprints, d.selection, mode);
  if (!items.length) {
    st().setHint("Nothing to move (locked footprints stay put)");
    return;
  }
  if (await moveFootprints(d, items)) scheduleAnalysis(d.info.id, 0);
}

/** U: the selected tracks' segment chain; again on the same chain, the whole connection up to the pads. */
export function selectConnection() {
  const d = activeDoc();
  if (d?.info.kind !== "pcb") return;
  const start = [...d.selection].filter((id) => ["track", "arcTrack", "via"].includes(d.items.get(id)?.kind ?? ""));
  if (!start.length) {
    st().setHint("Select a track or via first, then press U");
    return;
  }
  const chain = walkCopper(d.items, d.footprints, start, "chain");
  const same = chain.size === d.selection.size && [...chain].every((id) => d.selection.has(id));
  const ids = same ? walkCopper(d.items, d.footprints, start, "connection") : chain;
  st().patchDoc(d.info.id, { selection: ids });
  st().setHint(same ? `Connection: ${ids.size} track${ids.size === 1 ? "" : "s"} and vias selected (I selects the whole net)` : `${ids.size} segment${ids.size === 1 ? "" : "s"} up to the next pad, via or junction · U again expands to the connection`);
}

/** Deletes the routing of the selected footprints up to the first junction, pour or other pad, and refills. */
export async function unrouteSelected() {
  const d = activeDoc();
  if (d?.info.kind !== "pcb") return;
  const ids = unrouteFootprints(d.items, d.footprints, d.selection);
  if (!ids.length) {
    st().setHint("The selected footprints have no routing of their own");
    return;
  }
  if (await execute(d.info.id, { op: "route", delete: ids, add: [] })) {
    scheduleAnalysis(d.info.id, 0);
    toast.success(`Unrouted: ${ids.length} track${ids.length === 1 ? "" : "s"} and vias removed`);
  }
}

/** P P: packs the selected footprints into tight rows and attaches them to the cursor. */
export async function packAndMove() {
  const d = activeDoc();
  if (d?.info.kind !== "pcb") return;
  const ids = [...d.selection].filter((id) => { const it = d.items.get(id); return it?.kind === "footprint" && !it.locked; });
  if (!ids.length) {
    st().setHint("Select the footprints to pack first (locked ones stay put)");
    return;
  }
  const items = packFootprints(d.items, d.footprints, ids, Math.max(NM_PER_MM, d.grid), d.grid || 1);
  if (items.length) {
    const ok = await moveFootprints(d, items);
    if (!ok) return;
  }
  bus.emit("pickUp", { docId: d.info.id, ids });
}
