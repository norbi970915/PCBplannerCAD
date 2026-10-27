import { ask, open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
import { api, type DesignSource, type LibrarySource } from "./api";
import { bus } from "./bus";
import { selectable } from "./selectFilter";
import { scheduleAnalysis } from "./pcbActions";
import { activeDoc, useEditor, type MessageTarget } from "./store";
import type { Command, DocKind, ItemId, Point } from "./types";

const st = () => useEditor.getState();

export function report(e: unknown) {
  st().log("error", String(e));
}

const FILTERS = {
  schematic: [{ name: "PCBPlannerCAD Schematic", extensions: ["arsch"] }],
  pcb: [{ name: "PCBPlannerCAD PCB", extensions: ["arpcb"] }],
  library: [{ name: "PCBPlannerCAD Library", extensions: ["arlib"] }],
  project: [{ name: "PCBPlannerCAD Project", extensions: ["arproj"] }],
  any: [{ name: "PCBPlannerCAD files", extensions: ["arproj", "arsch", "arpcb", "arlib"] }],
};

// ---------- documents ----------

export async function showSnapshotDoc(id: string) {
  st().loadDoc(await api.snapshot(id));
  bus.emit("showDoc", { docId: id });
  afterEdit(id);
}

export async function execute(docId: string, cmd: Command): Promise<boolean> {
  try {
    const u = await api.execute(docId, cmd);
    st().applyUpdate(u);
    if (u.touchedLibrary) libraryTouched(u.touchedLibrary);
    afterEdit(docId);
    return true;
  } catch (e) {
    report(e);
    return false;
  }
}

/** Board documents refresh their ratsnest after every change. */
function afterEdit(docId: string) {
  if (st().docs[docId]?.info.kind === "pcb") scheduleAnalysis(docId);
}

let libRefreshTimer: number | undefined;
function libraryTouched(libId: string) {
  window.clearTimeout(libRefreshTimer);
  libRefreshTimer = window.setTimeout(() => {
    st().bumpLib(libId);
    void refreshLibs();
  }, 250);
}

export async function undo(docId = st().activeDocId, preserveRouting = false) {
  const doc = docId ? st().docs[docId] : undefined;
  if (!doc?.info.canUndo || !docId) return;
  if (!preserveRouting && ["route","diffPair"].includes(doc.tool)) st().patchDoc(docId,{tool:"select"});
  const u = await api.undo(docId);
  st().applyUpdate(u);
  if (u.touchedLibrary) libraryTouched(u.touchedLibrary);
  afterEdit(docId);
}

export async function redo(docId = st().activeDocId) {
  const doc = docId ? st().docs[docId] : undefined;
  if (!doc?.info.canRedo || !docId) return;
  if (["route","diffPair"].includes(doc.tool)) st().patchDoc(docId,{tool:"select"});
  const u = await api.redo(docId);
  st().applyUpdate(u);
  if (u.touchedLibrary) libraryTouched(u.touchedLibrary);
  afterEdit(docId);
}

export async function deleteSelection() {
  const doc = activeDoc();
  if (doc && doc.selection.size) await execute(doc.info.id, { op: "delete", ids: [...doc.selection] });
}

export function selectAll() {
  const doc = activeDoc();
  if (doc) st().patchDoc(doc.info.id, { selection: new Set([...doc.items].filter(([, item]) => item.kind !== "board" && selectable(doc.info.kind, item)).map(([id]) => id)) });
}

export function clearSelection() {
  const doc = activeDoc();
  if (doc) st().patchDoc(doc.info.id, { selection: new Set(), marked: new Set() });
}

export async function newDocument(kind: DocKind) {
  try {
    const s = await api.newDocument(kind);
    st().loadDoc(s);
    bus.emit("showDoc", { docId: s.info.id });
  } catch (e) {
    report(e);
  }
}

/** Opens any PCBPlannerCAD file, picking the right handler by extension. */
export async function openAnyFile(path?: string) {
  const p = path ?? (await openDialog({ filters: FILTERS.any, multiple: false, directory: false }));
  if (typeof p !== "string") return;
  const ext = p.split(".").pop()?.toLowerCase();
  if (ext === "arproj") return openProject(p);
  if (ext === "arlib") return openLibrary(p);
  try {
    const s = await api.open(p);
    st().loadDoc(s);
    bus.emit("showDoc", { docId: s.info.id });
    st().log("info", `Opened ${p}`);
  } catch (e) {
    report(e);
  }
}

export async function saveDoc(docId = st().activeDocId, saveAs = false): Promise<boolean> {
  const doc = docId ? st().docs[docId] : undefined;
  if (!doc || !docId) return false;
  try {
    if (doc.info.library) {
      const lib = st().libs.find((l) => l.id === doc.info.library);
      return saveLibrary(doc.info.library, saveAs || !lib?.path);
    }
    let path = doc.info.path;
    if (!path || saveAs) {
      const kind = doc.info.kind === "schematic" ? "schematic" : "pcb";
      path = await saveDialog({ filters: FILTERS[kind], defaultPath: path ?? doc.info.title });
      if (!path) return false;
    }
    st().setInfo(await api.save(docId, path));
    st().log("info", `Saved ${path}`);
    return true;
  } catch (e) {
    report(e);
    return false;
  }
}

export async function saveAll() {
  try {
    for (const info of await api.projectSaveAll()) st().setInfo(info);
    // Untitled documents and unsaved libraries need a file name each.
    for (const doc of Object.values(st().docs)) if (doc.info.dirty && !doc.info.library) await saveDoc(doc.info.id);
    await refreshLibs();
    await refreshOpenDocInfos();
    st().log("info", "Saved all documents.");
  } catch (e) {
    report(e);
  }
}

async function refreshOpenDocInfos() {
  for (const info of await api.docList()) st().setInfo(info);
}

/** Closes a document tab; asks first when it has unsaved changes. */
export async function closeDoc(docId: string): Promise<boolean> {
  const doc = st().docs[docId];
  if (doc && doc.info.dirty && !doc.info.library) {
    const discard = await ask(`${doc.info.title} has unsaved changes. Discard them?`, {
      title: "PCBPlannerCAD",
      kind: "warning",
      okLabel: "Discard",
      cancelLabel: "Cancel",
    });
    if (!discard) return false;
  }
  try {
    await api.close(docId);
  } catch (e) {
    report(e);
  }
  st().removeDoc(docId);
  return true;
}

export async function anyUnsaved(): Promise<string[]> {
  const docs = (await api.docList()).filter((d) => d.dirty && !d.library).map((d) => d.title);
  const libs = (await api.libList()).filter((l) => l.dirty && !l.readonly).map((l) => l.name);
  return [...docs, ...libs];
}

// ---------- selection transforms ----------

function selectionCenter(): Point | null {
  const doc = activeDoc();
  if (!doc || !doc.selection.size) return null;
  const c = st().cursor;
  if (c) return c;
  return null;
}

export async function rotateSelection(angle = 90) {
  const doc = activeDoc();
  const center = selectionCenter();
  if (doc && center) await execute(doc.info.id, { op: "rotate", ids: [...doc.selection], center, angle });
}

export async function mirrorSelection() {
  const doc = activeDoc();
  const center = selectionCenter();
  if (doc && center) await execute(doc.info.id, { op: "mirror", ids: [...doc.selection], center });
}

// ---------- libraries ----------

export async function refreshLibs() {
  try {
    st().setLibs(await api.libList());
  } catch (e) {
    report(e);
  }
}

export async function newLibrary(name: string) {
  try {
    const info = await api.libNew(name);
    await refreshLibs();
    bus.emit("showLibrary", { libId: info.id });
  } catch (e) {
    report(e);
  }
}

export async function openLibrary(path?: string) {
  const p = path ?? (await openDialog({ filters: FILTERS.library, multiple: false, directory: false }));
  if (typeof p !== "string") return;
  try {
    const info = await api.libOpen(p);
    await refreshLibs();
    bus.emit("showLibrary", { libId: info.id });
    st().log("info", `Opened library ${info.name} (${info.components} components, ${info.footprints} footprints).`);
  } catch (e) {
    report(e);
  }
}

export async function saveLibrary(libId: string, saveAs = false): Promise<boolean> {
  const lib = st().libs.find((l) => l.id === libId);
  try {
    let path: string | null = null;
    if (saveAs || !lib?.path || lib.readonly) {
      path = await saveDialog({ filters: FILTERS.library, defaultPath: `${lib?.name ?? "Library"}.arlib` });
      if (!path) return false;
    }
    const info = await api.libSave(libId, path);
    await refreshLibs();
    await refreshOpenDocInfos();
    st().log("info", `Saved library ${info.name} to ${info.path}`);
    return true;
  } catch (e) {
    report(e);
    return false;
  }
}

export async function closeLibrary(libId: string) {
  const lib = st().libs.find((l) => l.id === libId);
  if (lib?.dirty) {
    const ok = await ask(`Library ${lib.name} has unsaved changes. Close anyway?`, { title: "PCBPlannerCAD", kind: "warning" });
    if (!ok) return;
  }
  try {
    const closed = await api.libClose(libId);
    for (const id of closed) st().removeDoc(id);
    bus.emit("docsClosed", { docIds: closed });
    bus.emit("libraryClosed", { libId });
    await refreshLibs();
  } catch (e) {
    report(e);
  }
}

export async function openSymbolEditor(libId: string, name: string) {
  try {
    const infos = await api.openSymbol(libId, name);
    for (const info of infos) st().loadDoc(await api.snapshot(info.id));
    bus.emit("showSymbol", { libId, name });
  } catch (e) {
    report(e);
  }
}

export async function openFootprintEditor(libId: string, name: string) {
  try {
    const info = await api.openFootprint(libId, name);
    await showSnapshotDoc(info.id);
  } catch (e) {
    report(e);
  }
}

// ---------- project ----------

export async function refreshProject() {
  try {
    const p = await api.projectInfo();
    st().setProject(p.project);
  } catch (e) {
    report(e);
  }
}

async function applyProjectState(p: { project: import("./types").ProjectInfo | null; messages: string[] }) {
  st().setProject(p.project);
  for (const m of p.messages) st().log("warning", m);
  for (const s of p.project?.sheets ?? []) if (s.docId) st().loadDoc(await api.snapshot(s.docId));
  await refreshLibs();
}

export async function newProject() {
  const path = await saveDialog({ filters: FILTERS.project, defaultPath: "MyProject.arproj" });
  if (!path) return;
  try {
    await applyProjectState(await api.projectNew(path));
    const first = st().project?.sheets[0]?.docId;
    if (first) bus.emit("showDoc", { docId: first });
    st().log("info", `Created project ${path}`);
  } catch (e) {
    report(e);
  }
}

export async function openProject(path?: string) {
  const p = path ?? (await openDialog({ filters: FILTERS.project, multiple: false, directory: false }));
  if (typeof p !== "string") return;
  try {
    await applyProjectState(await api.projectOpen(p));
    const first = st().project?.sheets[0]?.docId;
    if (first) bus.emit("showDoc", { docId: first });
    st().log("info", `Opened project ${p}`);
  } catch (e) {
    report(e);
  }
}

export async function closeProject() {
  try {
    const p = st().project;
    const before = [...(p?.sheets ?? []), ...(p?.pcb ? [p.pcb] : [])].map((s) => s.docId).filter((d): d is string => !!d);
    await applyProjectState(await api.projectClose());
    for (const id of before) st().removeDoc(id);
    bus.emit("docsClosed", { docIds: before });
  } catch (e) {
    report(e);
  }
}

export async function addSheet() {
  const project = st().project;
  if (!project) return report("Open or create a project first.");
  const dir = project.path.replace(/[\\/][^\\/]*$/, "");
  const path = await saveDialog({ filters: FILTERS.schematic, defaultPath: `${dir}\\Sheet${project.sheets.length + 1}.arsch` });
  if (!path) return;
  try {
    await applyProjectState(await api.projectAddSheet(path));
    const sheet = st().project?.sheets.find((s) => path.replace(/\\/g, "/").endsWith(s.file));
    if (sheet?.docId) bus.emit("showDoc", { docId: sheet.docId });
  } catch (e) {
    report(e);
  }
}

export async function addLibraryToProject(libId: string) {
  try {
    const lib = st().libs.find((l) => l.id === libId);
    if (lib && !lib.path && !(await saveLibrary(libId, true))) return;
    await applyProjectState(await api.projectAddLibrary(libId));
  } catch (e) {
    report(e);
  }
}

/** Compiles the project, refreshing ERC results in the Messages panel. */
export async function compileProject(quiet = false) {
  if (!st().project) return report("Open or create a project to run ERC.");
  try {
    const c = await api.compile();
    st().setCompiled(c);
    st().clearMessages("erc");
    for (const v of c.violations) {
      st().log(v.severity, `${v.sheet}: ${v.message}`, { source: "erc", target: { sheet: v.sheet, items: v.items, pos: v.pos } });
    }
    const errors = c.violations.filter((v) => v.severity === "error").length;
    const warnings = c.violations.length - errors;
    if (!quiet || c.violations.length) {
      st().log(errors ? "error" : warnings ? "warning" : "info", `ERC: ${errors} errors, ${warnings} warnings — ${c.parts.length} parts, ${c.nets.length} nets.`, { source: "erc" });
    }
    bus.emit("showPanel", { id: "messages" });
  } catch (e) {
    report(e);
  }
}

export async function annotate(all: boolean) {
  if (!st().project) return report("Open or create a project to annotate.");
  if (all && !(await ask("Renumber every designator in the project?", { title: "Annotate", kind: "warning" }))) return;
  try {
    const r = await api.annotate(all);
    for (const id of r.docs) st().loadDoc(await api.snapshot(id));
    st().log("info", `Annotation updated ${r.changed} parts.`);
  } catch (e) {
    report(e);
  }
}

export async function exportBom() {
  if (!st().project) return report("Open or create a project to export a BOM.");
  const path = await saveDialog({ filters: [{ name: "CSV", extensions: ["csv"] }], defaultPath: `${st().project?.name ?? "bom"}_BOM.csv` });
  if (!path) return;
  try {
    const n = await api.exportBom(path);
    st().log("info", `BOM with ${n} parts written to ${path}`);
  } catch (e) {
    report(e);
  }
}

/** Opens the sheet of a message target, selects its items and centers on it. */
export async function navigateTo(t: MessageTarget) {
  let docId = t.docId;
  if (!docId && t.sheet) docId = (await api.sheetDoc(t.sheet)) ?? undefined;
  if (!docId) return;
  if (!st().docs[docId]) st().loadDoc(await api.snapshot(docId));
  bus.emit("showDoc", { docId });
  const items = new Set<ItemId>(t.items ?? []);
  st().patchDoc(docId, { selection: items, marked: new Set(), focus: t.pos ? { pos: t.pos, seq: Date.now() } : null });
}

export async function highlightNet(docId: string, item: ItemId) {
  try {
    const ids = await api.connectedItems(docId, item);
    st().patchDoc(docId, { marked: new Set(ids) });
  } catch (e) {
    report(e);
  }
}

// ---------- import ----------

export async function importLibrary(source: LibrarySource, path?: string) {
  let p: string | null | string[] = path ?? null;
  if (!p) {
    const structured = await api.libraryPaths().catch(() => ({ symbols: null, footprints: null }));
    p =
      source === "structuredFootprints"
        ? await openDialog({ directory: true, defaultPath: structured.footprints ?? undefined, title: "Select a footprint library folder" })
        : await openDialog({
            filters:
              source === "structuredSymbols"
                ? [{ name: "Symbol library", extensions: ["kicad_sym"] }]
                : [{ name: "XML library", extensions: ["lbr"] }],
            defaultPath: source === "structuredSymbols" ? (structured.symbols ?? undefined) : undefined,
            multiple: false,
          });
  }
  if (typeof p !== "string") return;
  try {
    st().log("info", `Importing ${p}…`);
    const r = await api.importLibrary(source, p);
    logReport(r.report, "import");
    await refreshLibs();
    st().log(
      "info",
      `Imported ${r.library.name}: ${r.library.components} components, ${r.library.symbols} symbols, ${r.library.footprints} footprints. Save it (Library ▸ Save) to keep it.`,
    );
    bus.emit("showLibrary", { libId: r.library.id });
  } catch (e) {
    report(e);
  }
}

export async function importDesign(source: DesignSource) {
  const filters =
    source === "structuredProject"
      ? [{ name: "Structured project", extensions: ["kicad_pro"] }]
      : source === "structuredSchematic"
        ? [{ name: "Structured schematic", extensions: ["kicad_sch"] }]
        : [{ name: "XML schematic", extensions: ["sch"] }];
  const src = await openDialog({ filters, multiple: false, title: "Select the design to import" });
  if (typeof src !== "string") return;
  const dest = await openDialog({ directory: true, title: "Choose an empty folder for the PCBPlannerCAD project" });
  if (typeof dest !== "string") return;
  try {
    st().log("info", `Importing ${src}…`);
    const r = await api.importDesign(source, src, dest);
    logReport(r.report, "import");
    await applyProjectState(r.project);
    st().log("info", `Imported into ${dest} (${r.files.length} files). Run Project ▸ Compile to check connectivity.`);
    const first = st().project?.sheets[0]?.docId;
    if (first) bus.emit("showDoc", { docId: first });
  } catch (e) {
    report(e);
  }
}

function logReport(lines: { warning: boolean; text: string }[], source: string) {
  const max = 300;
  for (const l of lines.slice(0, max)) st().log(l.warning ? "warning" : "info", l.text, { source });
  if (lines.length > max) st().log("warning", `…and ${lines.length - max} more import notes.`, { source });
}

export async function loadInitialState() {
  try {
    await refreshLibs();
    await refreshProject();
  } catch (e) {
    report(`Core backend unavailable — run the app through Tauri (pnpm dev). ${e}`);
  }
}
