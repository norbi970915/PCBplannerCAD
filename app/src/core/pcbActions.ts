import { toast } from "sonner";
import { api } from "./api";
import { report } from "./actions";
import { bus } from "./bus";
import { useEditor } from "./store";
import type { ItemId } from "./types";

const st = () => useEditor.getState();
const timers = new Map<string, number>();

/** Refreshes the ratsnest (and online DRC) shortly after board edits. */
export function scheduleAnalysis(docId: string, delay = 200) {
  window.clearTimeout(timers.get(docId));
  timers.set(
    docId,
    window.setTimeout(async () => {
      try {
        const a = await api.pcbAnalysis(docId);
        st().patchDoc(docId, { airwires: a.airwires });
        const unrouted = a.stats.unrouted;
        st().setHint(unrouted ? `${unrouted} of ${a.stats.connections} connections unrouted` : st().hint);
      } catch {
        // Analysis is advisory; ignore transient errors (e.g. doc closed).
      }
    }, delay),
  );
}

export async function runDrc(docId = st().activeDocId) {
  if (!docId) return;
  try {
    const r = await api.pcbDrc(docId);
    st().patchDoc(docId, { drc: r.violations });
    st().clearMessages("drc");
    for (const e of r.ruleErrors) st().log("warning", `Rule: ${e}`, { source: "drc" });
    for (const v of r.violations) st().log(v.severity, v.message, { source: "drc", target: { docId, items: v.items, pos: v.pos } });
    const errors = r.violations.filter((v) => v.severity === "error").length;
    st().log(errors ? "error" : "info", `DRC: ${errors} errors, ${r.violations.length - errors} warnings · ${r.stats.unrouted} of ${r.stats.connections} connections unrouted.`, {
      source: "drc",
    });
    bus.emit("showPanel", { id: "messages" });
  } catch (e) {
    report(e);
  }
}

export async function repour(docId = st().activeDocId, zone: ItemId | null = null, quiet = false) {
  if (!docId) return;
  try {
    st().applyUpdate(await api.pcbRepour(docId, zone));
    scheduleAnalysis(docId, 0);
    if (!quiet) st().log("info", zone === null ? "Repoured all copper pours." : "Repoured the selected pour.");
  } catch (e) {
    report(e);
  }
}

export async function refreshFootprints(docId: string) {
  st().patchDoc(docId, { footprints: await api.pcbFootprints(docId) });
}

export async function newProjectBoard() {
  try {
    const p = await api.projectNewPcb();
    st().setProject(p.project);
    const id = p.project?.pcb?.docId;
    if (id) {
      st().loadDoc(await api.snapshot(id));
      bus.emit("showDoc", { docId: id });
      scheduleAnalysis(id, 0);
    }
  } catch (e) {
    report(e);
  }
}

/** Opens the project board, creating it on first use. */
export async function openProjectBoard() {
  const p = st().project;
  if (!p) return report("Open or create a project first.");
  const id = p.pcb?.docId;
  if (!id) return newProjectBoard();
  if (!st().docs[id]) {
    const t = toast.loading("Opening board…");
    try {
      st().loadDoc(await api.snapshot(id));
    } catch (e) {
      return report(e);
    } finally {
      toast.dismiss(t);
    }
  }
  bus.emit("showDoc", { docId: id });
  scheduleAnalysis(id, 0);
}

export async function highlightNetOnBoard(docId: string, net: string) {
  try {
    st().patchDoc(docId, { marked: new Set(await api.pcbNetItems(docId, net)) });
  } catch (e) {
    report(e);
  }
}

// ---------- cross-probing ----------

/** Selecting parts in a schematic marks their footprints on the board and vice versa. */
export function installCrossProbe(): () => void {
  let busy = false;
  return useEditor.subscribe((s, prev) => {
    if (busy || s.activeDocId === null) return;
    const doc = s.docs[s.activeDocId];
    const before = prev.docs[s.activeDocId];
    if (!doc || doc.selection === before?.selection) return;
    const designators = new Set<string>();
    for (const id of doc.selection) {
      const it = doc.items.get(id);
      if (it?.kind === "part") {
        const d = it.fields.find((f) => f.name === "Designator")?.value;
        if (d) designators.add(d);
      } else if (it?.kind === "footprint") designators.add(it.designator);
    }
    const targetKind = doc.info.kind === "schematic" ? "pcb" : doc.info.kind === "pcb" ? "schematic" : null;
    if (!targetKind) return;
    busy = true;
    try {
      for (const other of Object.values(s.docs)) {
        if (other.info.kind !== targetKind) continue;
        const marked = new Set<ItemId>();
        if (designators.size) {
          for (const [id, it] of other.items) {
            if (it.kind === "footprint" && designators.has(it.designator)) marked.add(id);
            if (it.kind === "part" && designators.has(it.fields.find((f) => f.name === "Designator")?.value ?? "")) marked.add(id);
          }
        }
        if (marked.size || other.marked.size) st().patchDoc(other.info.id, { marked });
      }
    } finally {
      busy = false;
    }
  });
}
