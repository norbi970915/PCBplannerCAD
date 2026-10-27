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
  if (await execute(d.info.id, { op: "replaceMany", items })) scheduleAnalysis(d.info.id, 0);
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

/** P P : packs the selected footprints into tight rows and attaches them to the cursor. */
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
    // Routed parts keep their tracks, as with M: each moves through the connected drag (one undo step).
    const pl = st().placement;
    const moves = items.map(({ id, item }) => {
      const from = d.items.get(id) as { pos: { x: number; y: number } }, to = item as { pos: { x: number; y: number } };
      return { id, delta: { x: to.pos.x - from.pos.x, y: to.pos.y - from.pos.y } };
    });
    const ok = pl.moveMode === "disconnect"
      ? await execute(d.info.id, { op: "replaceMany", items })
      : await api.pcbDragEach(d.info.id, { moves, mode: pl.moveMode, routeMode: violatorModeOf(st()), corner: pl.cornerStyle })
          .then((r) => execute(d.info.id, { op: "route", delete: r.delete, add: r.add, replace: r.replace }), (e) => { report(e); return false; });
    if (!ok) return;
  }
  bus.emit("pickUp", { docId: d.info.id, ids });
}
