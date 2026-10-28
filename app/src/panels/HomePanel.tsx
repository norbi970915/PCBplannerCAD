import { ArrowRight, BookOpen, CircuitBoard, FileCode2, FileInput, FolderOpen, History, Plus, Sparkles } from "lucide-react";
import type { ReactNode } from "react";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Kbd } from "@/components/ui/kbd";
import { importDesign, importLibrary, newDocument, newProject, openAnyFile, openProject } from "../core/actions";
import { useEditor } from "../core/store";
import { readPreference } from "../core/preferences";

const LAST_PROJECT_KEY = "pcbplannercad.lastProject";

export function rememberProject(path: string | null) {
  try {
    if (path) localStorage.setItem(LAST_PROJECT_KEY, path);
  } catch {
    // Convenience only.
  }
}

function lastProject(): string | null {
  try {
    const saved = readPreference(LAST_PROJECT_KEY);
    return saved?.replace(/^A:\\Workspaces\\AllRoute(?=\\|$)/i, "A:\\Workspaces\\pCAD") ?? null;
  } catch {
    return null;
  }
}

function Task({ icon, eyebrow, title, text, action, onClick }: { icon: ReactNode; eyebrow: string; title: string; text: string; action: string; onClick: () => void }) {
  return (
    <button
      onClick={onClick}
      className="group flex min-w-0 flex-col items-start rounded-[5px] border border-line-strong bg-field p-4 text-left shadow-[var(--ui-surface-shadow)] transition-colors hover:border-accent-ink hover:bg-hover"
    >
      <div className="mb-2 flex w-full items-center justify-between text-faint">
        <span className="eyebrow">{eyebrow}</span>
        <span className="text-accent-ink [&_svg]:size-4">{icon}</span>
      </div>
      <div className="text-[13px] font-semibold text-ink">{title}</div>
      <div className="mt-1 text-xs leading-relaxed text-muted-foreground">{text}</div>
      <div className="mt-auto flex items-center gap-1.5 pt-3 text-[11px] font-medium text-accent-ink">
        {action} <ArrowRight className="size-3.5 transition-transform group-hover:translate-x-0.5" />
      </div>
    </button>
  );
}

const SHORTCUTS: [string[], string][] = [
  [["P", "P"], "Place part"],
  [["P", "W"], "Wire"],
  [["P", "N"], "Net label"],
  [["P", "V"], "VCC / GND"],
  [["Space"], "Schematic rotate"],
  [["M"], "PCB move"],
  [["R"], "PCB rotate"],
  [["X"], "PCB route"],
  [["X"], "Schematic mirror"],
  [["Q"], "mm / mil"],
  [["G"], "Snap grid"],
  [["C", "C"], "Compile & ERC"],
  [["T", "A"], "Annotate"],
];

export function HomePanel() {
  const project = useEditor((s) => s.project);
  const last = lastProject();
  return (
    <div className="h-full overflow-auto bg-doc">
      <div className="mx-auto flex max-w-[1180px] flex-col gap-4 px-6 py-6">
        <Card className="gap-0 overflow-hidden rounded-[5px] border-line py-0 shadow-[var(--ui-surface-shadow)]">
          <div className="px-6 pt-6 pb-5">
            <div className="eyebrow mb-1">PCB design workbench</div>
            <h1 className="text-[25px] leading-tight font-[650] tracking-[-0.6px] text-ink">
              pCAD<span className="title-dot">.</span> Schematic, layout and manufacturing in one place.
            </h1>
            <p className="mt-2 max-w-[100ch] text-[13px] leading-relaxed text-muted-foreground">
              Capture schematics, build component libraries, route boards and generate fabrication files. Import existing projects, schematics and libraries — everything stays on this PC.
            </p>
            {!project && last && (
              <button onClick={() => void openProject(last)} className="mt-4 inline-flex items-center gap-2 rounded-[5px] border border-line bg-secondary px-3 py-1.5 text-xs hover:bg-[var(--ui-btn-hover)]">
                <History className="size-3.5 text-accent-ink" /> Reopen <span className="font-medium">{last.split(/[\\/]/).pop()}</span>
              </button>
            )}
            {project && (
              <p className="mt-3 text-xs text-muted-foreground">
                Project <span className="font-medium text-ink">{project.name}</span> is open — pick a sheet in the Projects panel.
              </p>
            )}
          </div>
          <div className="grid grid-cols-1 gap-3 border-t border-line bg-panel-head px-6 py-5 sm:grid-cols-2 xl:grid-cols-4">
            <Task icon={<Plus />} eyebrow="Start" title="New project" text="A project with a first schematic sheet; add libraries and a board as you go." action="Get started" onClick={() => void newProject()} />
            <Task icon={<FolderOpen />} eyebrow="Open" title="Open a file" text="Project, schematic, board or library (.arproj, .arsch, .arpcb, .arlib)." action="Browse" onClick={() => void openAnyFile()} />
            <Task icon={<FileInput />} eyebrow="Import project" title="Structured project" text="Hierarchical sheets, embedded libraries and the board." action="Import" onClick={() => void importDesign("structuredProject")} />
            <Task icon={<FileInput />} eyebrow="Import schematic" title="XML design" text="Import a schematic and its board from XML design files (.sch / .brd)." action="Import" onClick={() => void importDesign("xmlSchematic")} />
          </div>
        </Card>

        <div className="grid grid-cols-1 gap-4 lg:grid-cols-[2fr_1fr]">
          <Card className="gap-0 rounded-[5px] border-line py-0">
            <CardHeader className="flex h-9 flex-row items-center border-b border-line bg-panel-head px-4 py-0">
              <CardTitle className="text-xs font-semibold">Libraries &amp; scratch documents</CardTitle>
            </CardHeader>
            <CardContent className="grid grid-cols-1 gap-3 p-4 sm:grid-cols-2">
              <Task icon={<BookOpen />} eyebrow="Library" title="Import symbol library" text="Import a structured symbol library, or browse installed libraries in the Components panel." action="Choose file" onClick={() => void importLibrary("structuredSymbols")} />
              <Task icon={<BookOpen />} eyebrow="Library" title="Import XML library" text="Parts, symbols and packages from an .lbr file." action="Choose file" onClick={() => void importLibrary("xmlLibrary")} />
              <Task icon={<FileCode2 />} eyebrow="Scratch" title="Free schematic" text="A sheet outside any project — handy for sketching circuits." action="Create" onClick={() => void newDocument("schematic")} />
              <Task icon={<CircuitBoard />} eyebrow="Scratch" title="Free board" text="An empty PCB with a default 2-layer stack and design rules." action="Create" onClick={() => void newDocument("pcb")} />
            </CardContent>
          </Card>
          <Card className="gap-0 rounded-[5px] border-line py-0">
            <CardHeader className="flex h-9 flex-row items-center gap-2 border-b border-line bg-panel-head px-4 py-0">
              <Sparkles className="size-3.5 text-copper" />
              <CardTitle className="text-xs font-semibold">Keyboard</CardTitle>
            </CardHeader>
            <CardContent className="grid grid-cols-1 gap-y-1.5 p-4 text-xs">
              {SHORTCUTS.map(([keys, label]) => (
                <div key={label} className="flex items-center justify-between gap-3">
                  <span className="text-muted-foreground">{label}</span>
                  <span className="flex gap-1">
                    {keys.map((k, i) => (
                      <Kbd key={i}>{k}</Kbd>
                    ))}
                  </span>
                </div>
              ))}
            </CardContent>
          </Card>
        </div>
      </div>
    </div>
  );
}
