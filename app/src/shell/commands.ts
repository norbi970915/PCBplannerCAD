import { getCurrentWindow } from "@tauri-apps/api/window";
import * as actions from "../core/actions";
import * as clipboard from "../core/clipboard";
import { useTheme } from "../core/theme";
import * as pcb from "../core/pcbActions";
import * as board from "../pcb/boardActions";
import { openDialog } from "../pcb/dialogs";
import { bus } from "../core/bus";
import { boardTabs, FOOTPRINT_TABS } from "../core/layers";
import { activeDoc, useEditor } from "../core/store";
import type { DocKind } from "../core/types";
import { TOOLS } from "../editor/profiles";
import { activateTool, cycleLayerMode } from "../editor/toolActions";
import { openWizardFor } from "../library/wizards";
import { openFind } from "./FindDialog";
import { promptText } from "./modal";
import { openLocalHub } from "../project/LocalHub";
import { openImportWizard } from "../project/ImportWizard";
import {openRecovery} from "../project/Recovery";
import { openRevisions } from "../project/Revisions";

export interface AppCommand {
  label: string;
  /** Chord ("Ctrl+S"), single key ("Delete") or key sequence ("P L"). */
  keys?: string[];
  /** Shown in menus but handled elsewhere (e.g. by the canvas tool). */
  display?: string;
  run: () => unknown;
  enabled?: () => boolean;
  /** Only active for these document kinds. */
  when?: DocKind[];
}

const ed = () => useEditor.getState();
const kindIs = (...kinds: DocKind[]) => () => {
  const k = activeDoc()?.info.kind;
  return !!k && kinds.includes(k);
};
const hasDoc = () => !!activeDoc();
const hasProject = () => !!ed().project;

export const COMMANDS: Record<string, AppCommand> = {
  "file.newProject": { label: "New electronics design…", run: () => openLocalHub(true) },
  "file.localHub": { label: "Local project hub…", run: () => openLocalHub() },
  "file.recovery": {label:"Recovery copies…",run:openRecovery},
  "file.revisions": { label: "Project revisions…", run: openRevisions, enabled: hasProject },
  "import.wizard": { label: "Import wizard…", run: () => openImportWizard() },
  "file.openProject": { label: "Open Project…", run: () => actions.openProject() },
  "file.closeProject": { label: "Close Project", run: actions.closeProject, enabled: hasProject },
  "file.newSchematic": {
    label: "New Schematic",
    keys: ["Ctrl+N"],
    run: () => (ed().project ? actions.addSheet() : actions.newDocument("schematic")),
  },
  "file.newPcb": { label: "New PCB (free document)", run: () => actions.newDocument("pcb") },
  "file.newLibrary": {
    label: "New Library…",
    run: async () => {
      const name = await promptText("New library", "Library name", "MyLibrary");
      if (name) await actions.newLibrary(name);
    },
  },
  "file.open": { label: "Open…", keys: ["Ctrl+O"], run: () => actions.openAnyFile() },
  "file.save": { label: "Save", keys: ["Ctrl+S"], run: () => actions.saveDoc(), enabled: hasDoc },
  "file.saveAs": { label: "Save As…", keys: ["Ctrl+Shift+S"], run: () => actions.saveDoc(undefined, true), enabled: hasDoc },
  "file.saveCopy": {label:"Save Copy…",run:()=>actions.saveCopy(),enabled:hasDoc},
  "file.saveAll": { label: "Save All", keys: ["Ctrl+Alt+S"], run: actions.saveAll },
  "file.close": {
    label: "Close Document",
    keys: ["Ctrl+W"],
    run: async () => {
      const id = ed().activeDocId;
      if (id && (await actions.closeDoc(id))) bus.emit("docsClosed", { docIds: [id] });
    },
    enabled: hasDoc,
  },
  "import.structuredProject": { label: "Structured project…", run: () => openImportWizard() },
  "import.structuredSchematic": { label: "Structured schematic…", run: () => openImportWizard() },
  "import.structuredSymbols": { label: "Symbol library…", run: () => actions.importLibrary("structuredSymbols") },
  "import.structuredFootprints": { label: "Footprint folder…", run: () => actions.importLibrary("structuredFootprints") },
  "import.xmlSchematic": { label: "XML schematic (.sch)…", run: () => openImportWizard() },
  "import.xmlLibrary": { label: "XML library (.lbr)…", run: () => actions.importLibrary("xmlLibrary") },
  "file.exit": { label: "Exit", run: () => getCurrentWindow().close() },

  "edit.undo": { label: "Undo", keys: ["Ctrl+Z"], run: () => actions.undo(), enabled: () => !!activeDoc()?.info.canUndo },
  "edit.redo": { label: "Redo", keys: ["Ctrl+Y", "Ctrl+Shift+Z"], run: () => actions.redo(), enabled: () => !!activeDoc()?.info.canRedo },
  "edit.cut": { label: "Cut", keys: ["Ctrl+X"], run: clipboard.cutSelection, enabled: () => !!activeDoc()?.selection.size },
  "edit.copy": { label: "Copy", keys: ["Ctrl+C"], run: clipboard.copySelection, enabled: () => !!activeDoc()?.selection.size },
  "edit.paste": { label: "Paste", keys: ["Ctrl+V"], run: clipboard.paste, enabled: () => hasDoc() && clipboard.hasClip() },
  "edit.array": { label: "Create Array…", when: ["pcb"], run: () => openDialog("array"), enabled: () => !!activeDoc()?.selection.size },
  "edit.selectSimilar": { label: "Select Similar…", when: ["pcb"], run: () => openDialog("similar"), enabled: () => activeDoc()?.selection.size === 1 },
  "edit.duplicate": { label: "Duplicate", keys: ["Ctrl+D"], run: clipboard.duplicateSelection, enabled: () => !!activeDoc()?.selection.size },
  "edit.delete": { label: "Delete", keys: ["Delete", "E D"], run: actions.deleteSelection, enabled: () => !!activeDoc()?.selection.size },
  "edit.rotate": { label: "Rotate Selection", display: "Space", run: () => actions.rotateSelection(), enabled: () => !!activeDoc()?.selection.size },
  "edit.mirror": { label: "Mirror Selection", display: "X", run: actions.mirrorSelection, enabled: () => !!activeDoc()?.selection.size },
  "edit.selectAll": { label: "Select All", keys: ["Ctrl+A", "S A"], run: actions.selectAll, enabled: hasDoc },
  "edit.find": { label: "Find…", keys: ["Ctrl+F"], run: openFind, enabled: hasDoc },
  "edit.deselectAll": { label: "Deselect All", keys: ["X A"], run: actions.clearSelection, enabled: hasDoc },

  "view.fit": { label: "Fit All Objects", keys: ["Ctrl+PageDown", "V F"], run: () => bus.emit("view", { action: "fit" }), enabled: hasDoc },
  "view.fitSelection": { label: "Fit Selection", keys: ["V S"], run: () => bus.emit("view", { action: "fitSelection" }), enabled: hasDoc },
  "view.zoomIn": { label: "Zoom In", keys: ["PageUp"], run: () => bus.emit("view", { action: "zoomIn" }), enabled: hasDoc },
  "view.zoomOut": { label: "Zoom Out", keys: ["PageDown"], run: () => bus.emit("view", { action: "zoomOut" }), enabled: hasDoc },
  "view.toggleUnits": { label: "Toggle Units (mm / mil)", keys: ["Q"], run: () => ed().toggleUnits() },
  "view.gridSettings": { label: "Grid & Snapping…", run: () => openDialog("grid"), enabled: hasDoc },
  "view.gridNext": { label: "Next Snap Grid", keys: ["G"], run: () => ed().cycleGrid(1), enabled: hasDoc },
  "view.gridPrev": { label: "Previous Snap Grid", keys: ["Shift+G"], run: () => ed().cycleGrid(-1), enabled: hasDoc },
  "tools.measure": { label: "Measure Distance", keys: ["Ctrl+M"], when: ["pcb", "footprint"], run: () => activateTool("measure") },
  "view.pourFills": { label: "Show Pour Fills", keys: ["V P"], when: ["pcb"], run: () => ed().togglePourFills() },
  "view.singleLayer": {
    label: "Cycle Layer Display (All / Dimmed / Active Only)",
    keys: ["Shift+S"],
    when: ["pcb", "footprint"],
    run: () => {
      const d = activeDoc();
      if (d) cycleLayerMode(d.info.id);
    },
  },
  "view.layerNext": { label: "Next Layer", keys: ["+", "*"], when: ["pcb", "footprint"], run: () => cycleLayer(1) },
  "view.layerPrev": { label: "Previous Layer", keys: ["-"], when: ["pcb", "footprint"], run: () => cycleLayer(-1) },

  "view.theme": { label: "Toggle Light / Dark Theme", keys: ["Ctrl+Shift+T"], run: () => useTheme.getState().toggle() },

  "project.compile": { label: "Compile & Run ERC", keys: ["C C"], run: () => actions.compileProject(), enabled: hasProject },
  "project.annotate": { label: "Annotate (fill in R?, U?…)", keys: ["T A"], run: () => actions.annotate(false), enabled: hasProject },
  "project.annotateAll": { label: "Re-annotate Everything", run: () => actions.annotate(true), enabled: hasProject },
  "project.bom": { label: "Export BOM (CSV)…", run: actions.exportBom, enabled: hasProject },
  "project.addSheet": { label: "Add Sheet…", run: actions.addSheet, enabled: hasProject },
  "project.board": { label: "Open / Create Project Board", keys: ["D B"], run: pcb.openProjectBoard, enabled: hasProject },

  "design.updatePcb": { label: "Update PCB from Schematic…", keys: ["D U"], when: ["pcb"], run: () => openDialog("eco") },
  "design.layerStack": { label: "Layer Stack Manager…", keys: ["D K"], when: ["pcb"], run: () => openDialog("stack") },
  "design.rules": { label: "Rules & Net Classes…", keys: ["D R"], when: ["pcb"], run: () => openDialog("rules") },
  "design.impedance": { label: "Impedance Calculator…", run: () => openDialog("impedance") },
  "tools.drcResults": { label: "Browse DRC Results…", when: ["pcb"], run: () => openDialog("drcResults") },
  "tools.drc": { label: "Design Rule Check", keys: ["T D"], when: ["pcb"], run: () => pcb.runDrc() },
  "tools.repour": { label: "Repour All Polygons", keys: ["T G"], when: ["pcb"], run: () => pcb.repour() },
  "tools.tune": { label: "Length Tuning of Selected Tracks…", keys: ["T R"], when: ["pcb"], run: () => openDialog("tune"), enabled: () => !!activeDoc()?.selection.size },
  "tools.pours": { label: "Pour Manager…", when: ["pcb"], run: () => openDialog("pours") },
  "tools.cleanup": { label: "Cleanup Tracks & Vias…", when: ["pcb"], run: () => openDialog("cleanup") },
  "tools.teardrops": { label: "Teardrops…", when: ["pcb"], run: () => openDialog("teardrops") },
  "tools.stitch": { label: "Via Stitching…", when: ["pcb"], run: () => openDialog("stitch") },
  "tools.stats": { label: "Board Statistics & Drill Table…", when: ["pcb"], run: () => openDialog("stats") },
  "edit.selectConnection": { label: "Select Connection (again: whole connection)", keys: ["U"], when: ["pcb"], run: board.selectConnection },
  "edit.packMove": { label: "Pack & Move Footprints", keys: ["P P"], when: ["pcb"], run: board.packAndMove, enabled: board.canUnroute },
  "edit.unroute": { label: "Unroute Selected Footprints", when: ["pcb"], run: board.unrouteSelected, enabled: board.canUnroute },
  "edit.alignLeft": { label: "Align Left", when: ["pcb"], run: () => board.align("left"), enabled: board.canAlign },
  "edit.alignRight": { label: "Align Right", when: ["pcb"], run: () => board.align("right"), enabled: board.canAlign },
  "edit.alignTop": { label: "Align Top", when: ["pcb"], run: () => board.align("top"), enabled: board.canAlign },
  "edit.alignBottom": { label: "Align Bottom", when: ["pcb"], run: () => board.align("bottom"), enabled: board.canAlign },
  "edit.alignCenterX": { label: "Align Centers Vertically", when: ["pcb"], run: () => board.align("centerX"), enabled: board.canAlign },
  "edit.alignCenterY": { label: "Align Centers Horizontally", when: ["pcb"], run: () => board.align("centerY"), enabled: board.canAlign },
  "edit.distributeX": { label: "Distribute Horizontally", when: ["pcb"], run: () => board.align("distributeX"), enabled: board.canDistribute },
  "edit.distributeY": { label: "Distribute Vertically", when: ["pcb"], run: () => board.align("distributeY"), enabled: board.canDistribute },
  "design.toggleRules": {
    label: "Toggle Design Rules (keep clearances / ignore violators)",
    keys: ["Ctrl+Shift+D"],
    when: ["pcb"],
    run: () => {
      ed().toggleDrcEnforce();
      ed().setHint(ed().drcEnforce ? "Design rules ON: routing and moves walk around or push other nets" : "Design rules OFF: routing and moves ignore violators (online DRC still marks them)");
    },
  },
  "tools.onlineDrc": {
    label: "Toggle Online DRC (check after every change)",
    when: ["pcb"],
    run: () => {
      ed().toggleOnlineDrc();
      ed().setHint(ed().onlineDrc ? "Online DRC on: violations update after every change" : "Online DRC off: run T D to check");
    },
  },
  "tools.clearMarks": { label: "Clear DRC Markers & Highlights", keys: ["T M"], when: ["pcb"], run: () => {
      const d = activeDoc();
      if (d) ed().patchDoc(d.info.id, { drc: [], marked: new Set() });
    } },
  "view.3d": { label: "3D View", keys: ["3"], when: ["pcb"], run: () => {
      const d = activeDoc();
      if (d) bus.emit("show3d", { docId: d.info.id });
    } },
  "file.outputs": { label: "Fabrication Outputs…", keys: ["F F"], when: ["pcb"], run: () => openDialog("outputs") },
  "file.print": { label: "Print to PDF…", keys: ["Ctrl+P"], enabled: hasDoc, run: () => openDialog("print") },
  "file.gerberViewer": { label: "Open Gerber Viewer…", run: () => bus.emit("showGerbers", {}) },
  "tools.panelize": { label: "Panelize Board…", when: ["pcb"], run: () => openDialog("panelize") },
  "tools.scripts": { label: "Script Console", run: () => bus.emit("showPanel", { id: "scripts" }) },
  "project.variants": { label: "Variants…", run: () => openDialog("variants"), enabled: hasProject },

  "tools.footprintWizard": { label: "Footprint Wizard…", run: () => openWizardFor("footprint") },
  "tools.symbolWizard": { label: "Symbol Wizard…", run: () => openWizardFor("symbol") },

  "window.resetLayout": { label: "Reset Panel Layout", run: () => bus.emit("resetLayout", {}) },
  "window.projects": { label: "Projects Panel", run: () => bus.emit("showPanel", { id: "projects" }) },
  "window.components": { label: "Components Panel", run: () => bus.emit("showPanel", { id: "components" }) },
  "window.pcb": { label: "PCB Panel (Nets)", run: () => bus.emit("showPanel", { id: "pcb" }) },
  "window.properties": { label: "Properties Panel", run: () => bus.emit("showPanel", { id: "properties" }) },
  "window.messages": { label: "Messages Panel", run: () => bus.emit("showPanel", { id: "messages" }) },
  "help.canvasTimings": { label: "Record/Stop Canvas Timings", enabled: hasDoc, run: () => { const d=activeDoc(); if(d)bus.emit("canvasTimings",{docId:d.info.id}); } },
  "help.about": {
    label: "About pCAD",
    run: () => ed().log("info", "pCAD 0.6.0 — schematic capture, PCB layout, manufacturing outputs, 3D, and external design import."),
  },
};

// Place commands are generated per editor from the tool profiles.
for (const kind of Object.keys(TOOLS) as DocKind[]) {
  for (const t of TOOLS[kind]) {
    if (t.id === "select") continue;
    COMMANDS[`place.${kind}.${t.id}`] = { label: t.label, keys: [t.keys], when: [kind], run: () => activateTool(t.id) };
  }
}
COMMANDS["tool.select"] = { label: "Select", keys: ["S S"], run: () => activateTool("select"), enabled: hasDoc };

function cycleLayer(step: 1 | -1) {
  const d = activeDoc();
  if (!d) return;
  const tabs = d.info.kind === "footprint" ? FOOTPRINT_TABS : boardTabs(d.items.values()).map((t) => t.id);
  const i = tabs.indexOf(d.activeLayer);
  ed().patchDoc(d.info.id, { activeLayer: tabs[(i + step + tabs.length) % tabs.length] });
}

export type CommandId = string;

export function commandAvailable(id: CommandId): boolean {
  const cmd = COMMANDS[id];
  if (!cmd) return false;
  if (cmd.when && !kindIs(...cmd.when)()) return false;
  return !cmd.enabled || cmd.enabled();
}

export function runCommand(id: CommandId) {
  const cmd = COMMANDS[id];
  if (!cmd || !commandAvailable(id)) return;
  Promise.resolve(cmd.run()).catch((e) => ed().log("error", String(e)));
}
