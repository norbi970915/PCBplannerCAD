import { useEffect, useRef, type FunctionComponent } from "react";
import { ask } from "@tauri-apps/plugin-dialog";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  DockviewReact,
  themeDark,
  themeLight,
  type AddPanelOptions,
  type DockviewApi,
  type DockviewReadyEvent,
  type IDockviewPanelHeaderProps,
  type IDockviewPanelProps,
} from "dockview-react";
import { anyUnsaved, closeDoc } from "./core/actions";
import { bus } from "./core/bus";
import { useEditor } from "./core/store";
import { readPreference } from "./core/preferences";
import { useTheme } from "./core/theme";
import { X } from "lucide-react";
import { DocumentPanel, SymbolEditorPanel } from "./editor/DocumentPanel";
import { LibraryView } from "./library/LibraryView";
import { ComponentsPanel } from "./panels/ComponentsPanel";
import { HomePanel, rememberProject } from "./panels/HomePanel";
import { MessagesPanel } from "./panels/MessagesPanel";
import { PanelErrorBoundary } from "./panels/PanelErrorBoundary";
import { ProjectsPanel } from "./panels/ProjectsPanel";
import { PropertiesPanel } from "./panels/PropertiesPanel";
import { PcbPanel } from "./panels/PcbPanel";
import { ScriptsPanel } from "./panels/ScriptsPanel";
import { installCrossProbe, installOnlineDrc } from "./core/pcbActions";
import { GerberViewer } from "./view3d/GerberViewer";
import { View3DPanel } from "./view3d/View3DPanel";
import { Icons } from "./shell/icons";
import { MenuBar } from "./shell/MenuBar";
import { ContextMenu } from "./shell/ContextMenu";
import { KeyMenu } from "./shell/KeyMenu";
import { ModalHost } from "./shell/modal";
import { installShortcuts } from "./shell/shortcuts";
import { StatusBar } from "./shell/StatusBar";

const LAYOUT_KEY = "pcbplannercad.layout.v2";
const TOOL_PANELS = ["projects", "components", "properties", "messages"] as const;
const isDocPanel = (id: string) =>
  id === "home" || id.startsWith("doc:") || id.startsWith("sym:") || id.startsWith("lib:") || id.startsWith("view3d:") || id === "gerbers";

const st = () => useEditor.getState();

function guarded(name: string, Panel: FunctionComponent<IDockviewPanelProps>): FunctionComponent<IDockviewPanelProps> {
  return (props) => (
    <PanelErrorBoundary name={name}>
      <Panel {...props} />
    </PanelErrorBoundary>
  );
}

/** Keeps the store's active document in sync with the focused dock tab. */
function useActivate(props: IDockviewPanelProps, docId: () => string | undefined) {
  useEffect(() => {
    const sub = props.api.onDidActiveChange((e) => {
      const id = docId();
      if (e.isActive && id) st().setActiveDoc(id);
    });
    if (props.api.isActive) {
      const id = docId();
      if (id) st().setActiveDoc(id);
    }
    return () => sub.dispose();
  }, [props.api, docId]);
}

const components: Record<string, FunctionComponent<IDockviewPanelProps>> = {
  home: guarded("Home", () => <HomePanel />),
  projects: guarded("Projects", () => <ProjectsPanel />),
  components: guarded("Components", () => <ComponentsPanel />),
  properties: guarded("Properties", () => <PropertiesPanel />),
  messages: guarded("Messages", () => <MessagesPanel />),
  scripts: guarded("Scripts", () => <ScriptsPanel />),
  pcb: guarded("PCB", () => <PcbPanel />),
  view3d: guarded("3D view", (props) => {
    const docId = (props.params as { docId: string }).docId;
    useActivate(props, () => docId);
    return <View3DPanel docId={docId} />;
  }),
  gerbers: guarded("Gerber viewer", (props) => <GerberViewer initialPath={(props.params as { path?: string } | undefined)?.path} />),
  document: guarded("Document", (props) => {
    const docId = (props.params as { docId: string }).docId;
    useActivate(props, () => docId);
    return <DocumentPanel docId={docId} />;
  }),
  symbolEditor: guarded("Symbol editor", (props) => {
    const { libId, name } = props.params as { libId: string; name: string };
    useActivate(props, () =>
      Object.values(st().docs).find((d) => d.info.library === libId && d.info.entry === name && d.info.kind === "symbol" && d.info.id === st().activeDocId)?.info.id ??
      Object.values(st().docs).find((d) => d.info.library === libId && d.info.entry === name && d.info.kind === "symbol")?.info.id,
    );
    return <SymbolEditorPanel libId={libId} name={name} />;
  }),
  library: guarded("Library", (props) => <LibraryView libId={(props.params as { libId: string }).libId} />),
};

/** Document tab with a dirty marker and a close button that asks first. */
function DocTab(props: IDockviewPanelHeaderProps) {
  const id = props.api.id;
  const params = props.params as { docId?: string; libId?: string };
  const dirty = useEditor((s) =>
    params.docId ? (s.docs[params.docId]?.info.dirty ?? false) : params.libId ? (s.libs.find((l) => l.id === params.libId)?.dirty ?? false) : false,
  );
  const kind = useEditor((s) => (params.docId ? s.docs[params.docId]?.info.kind : undefined));
  const icon = id.startsWith("lib:")
    ? "library"
    : id.startsWith("sym:")
      ? "symbol"
      : id === "home"
        ? "folder"
        : kind === "pcb"
          ? "board"
          : kind === "footprint"
            ? "footprint"
            : kind === "schematic"
              ? "schematic"
              : id.startsWith("view3d:")
                ? "model3d"
                : id === "gerbers"
                  ? "board"
                  : undefined;
  const close = async (e: React.MouseEvent) => {
    e.stopPropagation();
    if (params.docId && !(await closeDoc(params.docId))) return;
    props.api.close();
  };
  return (
    <div className="group flex h-full items-center gap-1.5 pr-1 pl-2.5 text-xs whitespace-nowrap" onMouseDown={(e) => e.button === 1 && void close(e)}>
      {icon && <span className="flex text-faint [&_svg]:size-3.5">{Icons[icon]}</span>}
      <span>{props.api.title}</span>
      {dirty && <span className="dirty-dot" />}
      <button
        className="grid size-4.5 place-items-center rounded-[3px] text-faint opacity-60 hover:bg-hover hover:text-ink hover:opacity-100 group-hover:opacity-100"
        onClick={(e) => void close(e)}
        title="Close"
      >
        <X className="size-3" />
      </button>
    </div>
  );
}

function ToolTab(props: IDockviewPanelHeaderProps) {
  return <div className="flex h-full items-center px-2.5 text-xs font-medium whitespace-nowrap">{props.api.title}</div>;
}

const tabComponents = { doc: DocTab, tool: ToolTab };

const PANEL_DEFAULTS: Record<(typeof TOOL_PANELS)[number] | "scripts" | "pcb", { title: string }> = {
  projects: { title: "Projects" },
  components: { title: "Components" },
  properties: { title: "Properties" },
  messages: { title: "Messages" },
  scripts: { title: "Scripts" },
  pcb: { title: "PCB" },
};

function buildDefaultLayout(api: DockviewApi) {
  api.clear();
  api.addPanel({ id: "home", component: "home", title: "Home", tabComponent: "doc" });
  api.addPanel({ id: "projects", component: "projects", title: "Projects", tabComponent: "tool", position: { referencePanel: "home", direction: "left" }, initialWidth: 270 });
  api.addPanel({ id: "components", component: "components", title: "Components", tabComponent: "tool", position: { referencePanel: "projects", direction: "within" }, inactive: true });
  api.addPanel({ id: "pcb", component: "pcb", title: "PCB", tabComponent: "tool", position: { referencePanel: "projects", direction: "within" }, inactive: true });
  api.addPanel({ id: "properties", component: "properties", title: "Properties", tabComponent: "tool", position: { referencePanel: "home", direction: "right" }, initialWidth: 310 });
  api.addPanel({ id: "messages", component: "messages", title: "Messages", tabComponent: "tool", position: { referencePanel: "home", direction: "below" }, initialHeight: 170 });
  api.getPanel("home")?.api.setActive();
}

function restoreLayout(api: DockviewApi): boolean {
  try {
    const saved = readPreference(LAYOUT_KEY);
    if (!saved) return false;
    api.fromJSON(JSON.parse(saved));
    // Documents aren't reopened across restarts; drop their tabs.
    for (const p of [...api.panels]) if (isDocPanel(p.id) && p.id !== "home") p.api.close();
    if (!TOOL_PANELS.every((id) => api.getPanel(id))) return false;
    if (!api.getPanel("home")) addDocPanel(api, { id: "home", component: "home", title: "Home", tabComponent: "doc" });
    return true;
  } catch {
    return false;
  }
}

/** Adds a panel to the document area (next to other document tabs). */
type DocPanelOptions = Pick<AddPanelOptions, "id" | "component" | "title" | "params" | "tabComponent">;

function addDocPanel(api: DockviewApi, options: DocPanelOptions) {
  const ref = api.panels.find((p) => isDocPanel(p.id));
  const position = ref ? { referencePanel: ref.id, direction: "within" as const } : { referencePanel: "properties", direction: "left" as const };
  return api.addPanel({ ...options, position });
}

function showOrAdd(api: DockviewApi, options: DocPanelOptions) {
  const existing = api.getPanel(options.id);
  if (existing) existing.api.setActive();
  else addDocPanel(api, options);
}

export function App() {
  const resolved = useTheme((s) => s.resolved);
  const apiRef = useRef<DockviewApi | null>(null);

  const onReady = (event: DockviewReadyEvent) => {
    const api = event.api;
    apiRef.current = api;
    if (!restoreLayout(api)) buildDefaultLayout(api);
    api.onDidLayoutChange(() => {
      try {
        localStorage.setItem(LAYOUT_KEY, JSON.stringify(api.toJSON()));
      } catch {
        // Layout persistence is a convenience; ignore storage failures.
      }
    });
  };

  useEffect(() => {
    const api = () => apiRef.current;
    const offs = [
      installShortcuts(),
      installCrossProbe(),
      installOnlineDrc(),
      bus.on("show3d", ({ docId }) => {
        const a = api();
        const doc = st().docs[docId];
        if (!a || !doc) return;
        showOrAdd(a, { id: `view3d:${docId}`, component: "view3d", title: `3D · ${doc.info.title}`, params: { docId }, tabComponent: "doc" });
      }),
      bus.on("showGerbers", ({ path }) => {
        const a = api();
        if (a) showOrAdd(a, { id: "gerbers", component: "gerbers", title: "Gerber viewer", params: { path }, tabComponent: "doc" });
      }),
      bus.on("showDoc", ({ docId }) => {
        const a = api();
        const doc = st().docs[docId];
        if (!a || !doc) return;
        showOrAdd(a, { id: `doc:${docId}`, component: "document", title: doc.info.title, params: { docId }, tabComponent: "doc" });
        st().setActiveDoc(docId);
      }),
      bus.on("showSymbol", ({ libId, name }) => {
        const a = api();
        if (a) showOrAdd(a, { id: `sym:${libId}:${name}`, component: "symbolEditor", title: name, params: { libId, name }, tabComponent: "doc" });
      }),
      bus.on("showLibrary", ({ libId }) => {
        const a = api();
        const lib = st().libs.find((l) => l.id === libId);
        if (a) showOrAdd(a, { id: `lib:${libId}`, component: "library", title: lib?.name ?? "Library", params: { libId }, tabComponent: "doc" });
      }),
      bus.on("docsClosed", ({ docIds }) => {
        for (const id of docIds) {
          api()?.getPanel(`doc:${id}`)?.api.close();
          api()?.getPanel(`view3d:${id}`)?.api.close();
        }
      }),
      bus.on("libraryClosed", ({ libId }) => {
        for (const p of [...(api()?.panels ?? [])]) if (p.id === `lib:${libId}` || p.id.startsWith(`sym:${libId}:`)) p.api.close();
      }),
      bus.on("resetLayout", () => {
        const a = api();
        if (!a) return;
        const docs = a.panels.filter((p) => p.id.startsWith("doc:")).map((p) => p.params as { docId: string });
        buildDefaultLayout(a);
        for (const d of docs) bus.emit("showDoc", d);
      }),
      bus.on("showPanel", ({ id }) => {
        const a = api();
        if (!a) return;
        const existing = a.getPanel(id);
        if (existing) return existing.api.setActive();
        if (id === "pcb" && a.getPanel("projects")) {
          a.addPanel({ id, component: id, title: PANEL_DEFAULTS[id].title, tabComponent: "tool", position: { referencePanel: "projects", direction: "within" } });
          return;
        }
        if (id === "scripts" && a.getPanel("messages")) {
          a.addPanel({ id, component: id, title: PANEL_DEFAULTS[id].title, tabComponent: "tool", position: { referencePanel: "messages", direction: "within" } });
          return;
        }
        a.addPanel({ id, component: id, title: PANEL_DEFAULTS[id].title, tabComponent: "tool", floating: { width: 320, height: 420 } });
      }),
    ];

    // Tab titles follow document/library names and dirty state.
    const offTitles = useEditor.subscribe((s, prev) => {
      const a = api();
      if (!a) return;
      if (s.docs !== prev.docs) {
        for (const d of Object.values(s.docs)) {
          const p = a.getPanel(`doc:${d.info.id}`);
          if (p && p.api.title !== d.info.title) p.api.setTitle(d.info.title);
        }
      }
      if (s.libs !== prev.libs) {
        for (const l of s.libs) {
          const p = a.getPanel(`lib:${l.id}`);
          if (p && p.api.title !== l.name) p.api.setTitle(l.name);
        }
      }
      if (s.project?.path !== prev.project?.path) rememberProject(s.project?.path ?? null);
      const active = s.activeDocId ? s.docs[s.activeDocId] : undefined;
      const title = `${s.project ? `${s.project.name} — ` : ""}${active ? active.info.title : ""}${active?.info.dirty ? " *" : ""}`;
      void getCurrentWindow().setTitle(`${title ? `${title} — ` : ""}PCBPlannerCAD`).catch(() => {});
    });

    // The close listener registers asynchronously; if the effect is torn down
    // first (React StrictMode does this in dev), unregister when it arrives.
    let disposed = false;
    let unlistenClose: (() => void) | undefined;
    void getCurrentWindow()
      .onCloseRequested(async (event) => {
        const unsaved = await anyUnsaved().catch(() => []);
        if (!unsaved.length) return;
        const ok = await ask(`Unsaved changes in: ${unsaved.slice(0, 6).join(", ")}${unsaved.length > 6 ? "…" : ""}. Quit anyway?`, {
          title: "PCBPlannerCAD",
          kind: "warning",
          okLabel: "Quit",
          cancelLabel: "Cancel",
        });
        if (!ok) event.preventDefault();
      })
      .then((fn) => (disposed ? fn() : (unlistenClose = fn)))
      .catch(() => {});

    return () => {
      disposed = true;
      offs.forEach((off) => off());
      offTitles();
      unlistenClose?.();
    };
  }, []);

  return (
    <div className="app">
      <MenuBar />
      <div className="dock">
        <DockviewReact components={components} tabComponents={tabComponents} onReady={onReady} theme={resolved === "dark" ? themeDark : themeLight} />
      </div>
      <StatusBar />
      <ModalHost />
      <KeyMenu />
      <ContextMenu />
    </div>
  );
}
