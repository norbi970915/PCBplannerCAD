import { Box, Factory, FileInput, FolderOpen, FolderPlus, Layers, ListChecks, Plus, RefreshCcw, Save, ShieldCheck, Table2, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { Panel, PanelHeading, Section, CollapsibleSection, TreeRow } from "@/components/app/panel";
import {
  addSheet,
  annotate,
  closeProject,
  compileProject,
  exportBom,
  openProject,
  saveAll,
} from "../core/actions";
import { bus } from "../core/bus";
import { newProjectBoard, openProjectBoard } from "../core/pcbActions";
import { openDialog } from "../pcb/dialogs";
import { useEditor } from "../core/store";
import { Icons } from "../shell/icons";

import { useState } from "react";
import { LocalHub, openLocalHub } from "../project/LocalHub";
import { openRevisions } from "../project/Revisions";
import { openImportWizard } from "../project/ImportWizard";

const basename = (p: string) => p.split(/[\\/]/).pop() ?? p;

function IconAction({ label, onClick, children }: { label: string; onClick: () => void; children: React.ReactNode }) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Button variant="ghost" size="icon-sm" onClick={onClick}>
          {children}
        </Button>
      </TooltipTrigger>
      <TooltipContent>{label}</TooltipContent>
    </Tooltip>
  );
}

export function ProjectsPanel() {
  const project = useEditor((s) => s.project);
  const [tab,setTab]=useState<"files"|"project">("project");
  const docs = useEditor((s) => s.docs);
  const libs = useEditor((s) => s.libs);
  const active = useEditor((s) => s.activeDocId);
  const projectLibraries=libs.filter(l=>l.inProject);

  const projectDocIds = new Set(project?.sheets.map((s) => s.docId) ?? []);
  const freeDocs = Object.values(docs).filter((d) => !projectDocIds.has(d.info.id) && !d.info.library && !d.info.inProject);
  const boardDoc = project?.pcb?.docId ? docs[project.pcb.docId] : undefined;

  if(tab==="files")return <Panel><PanelHeading title="Local files"><Button variant="ghost" size="sm" onClick={()=>setTab("project")}>Project</Button></PanelHeading><div className="min-h-0 flex-1 overflow-auto"><LocalHub/></div></Panel>;
  return (
    <Panel>
      <PanelHeading title={project ? project.name : "Projects"}>
        <Button variant="ghost" size="sm" onClick={()=>setTab("files")}>Files</Button>
        {project ? (
          <>
            <IconAction label="Compile & ERC (C C)" onClick={() => void compileProject()}>
              <ShieldCheck />
            </IconAction>
            <IconAction label="Save all (Ctrl+Alt+S)" onClick={() => void saveAll()}>
              <Save />
            </IconAction>
            <IconAction label="Close project" onClick={() => void closeProject()}>
              <X />
            </IconAction>
          </>
        ) : (
          <IconAction label="Open project…" onClick={() => void openProject()}>
            <FolderOpen />
          </IconAction>
        )}
      </PanelHeading>
      <div className="min-h-0 flex-1 overflow-auto">
        {!project ? (
          <Section title="Get started">
            <div className="grid gap-1.5">
              <Button variant="outline" className="justify-start" onClick={() => openLocalHub(true)}>
                <FolderPlus /> New project…
              </Button>
              <Button variant="outline" className="justify-start" onClick={() => void openProject()}>
                <FolderOpen /> Open project…
              </Button>
              <Button variant="outline" className="justify-start" onClick={() => openImportWizard()}>
                <FileInput /> Import structured project…
              </Button>
              <Button variant="outline" className="justify-start" onClick={() => openImportWizard()}>
                <FileInput /> Import XML design…
              </Button>
            </div>
          </Section>
        ) : (
          <>
            <Section
              title="Source documents"
              actions={
                <IconAction label="Add sheet…" onClick={() => void addSheet()}>
                  <Plus />
                </IconAction>
              }
            >
              {project.sheets.map((s) => {
                const d = s.docId ? docs[s.docId] : undefined;
                return (
                  <TreeRow
                    key={s.file}
                    icon={Icons.schematic}
                    active={!!s.docId && s.docId === active}
                    onClick={s.docId ? () => bus.emit("showDoc", { docId: s.docId! }) : undefined}
                    title={s.file}
                    trailing={d?.info.dirty ? <span className="dirty-dot" /> : !s.docId ? <Badge variant="destructive">missing</Badge> : null}
                  >
                    {basename(s.file)}
                  </TreeRow>
                );
              })}
            </Section>
            <Section
              title="Board"
              actions={
                !project.pcb && (
                  <IconAction label="New board" onClick={() => void newProjectBoard()}>
                    <Plus />
                  </IconAction>
                )
              }
            >
              {project.pcb ? (
                <TreeRow
                  icon={Icons.board}
                  active={!!project.pcb.docId && project.pcb.docId === active}
                  onClick={() => void openProjectBoard()}
                  title={project.pcb.file}
                  trailing={boardDoc?.info.dirty ? <span className="dirty-dot" /> : !project.pcb.docId ? <Badge variant="destructive">missing</Badge> : null}
                >
                  {basename(project.pcb.file)}
                </TreeRow>
              ) : (
                <div className="grid gap-1.5 px-1 py-0.5">
                  <p className="text-muted-foreground">No board yet.</p>
                  <Button variant="outline" size="sm" className="w-fit" onClick={() => void newProjectBoard()}>
                    <Plus /> New board
                  </Button>
                </div>
              )}
            </Section>
            <CollapsibleSection title="Project libraries" count={projectLibraries.length} dirty={projectLibraries.some(l=>l.dirty)} preferenceKey="pcbplannercad.sidebar.projectLibrariesExpanded">
              {projectLibraries
                .map((l) => (
                  <TreeRow key={l.id} icon={Icons.library} onClick={() => bus.emit("showLibrary", { libId: l.id })} trailing={l.dirty ? <span className="dirty-dot" /> : null}>
                    {l.name}
                  </TreeRow>
                ))}
              {!projectLibraries.length && <p className="px-2 py-1 text-muted-foreground">None yet — use “Add to Project” on a library tab.</p>}
            </CollapsibleSection>
            <Section title="Tools">
              <div className="flex flex-wrap gap-1.5">
                <Button variant="outline" size="sm" onClick={() => void annotate(false)}>
                  <ListChecks /> Annotate
                </Button>
                <Button variant="outline" size="sm" onClick={() => void compileProject()}>
                  <ShieldCheck /> Compile / ERC
                </Button>
                <Button variant="outline" size="sm" onClick={() => void exportBom()}>
                  <Table2 /> BOM…
                </Button>
                {project.pcb && (
                  <>
                    <Button variant="outline" size="sm" onClick={() => void openProjectBoard().then(() => openDialog("eco"))}>
                      <RefreshCcw /> Update PCB
                    </Button>
                    <Button variant="outline" size="sm" onClick={() => void openProjectBoard().then(() => openDialog("outputs"))}>
                      <Factory /> Fab outputs
                    </Button>
                    <Button variant="outline" size="sm" onClick={() => project.pcb?.docId && bus.emit("show3d", { docId: project.pcb.docId })}>
                      <Box /> 3D
                    </Button>
                  </>
                )}
                <Button variant="outline" size="sm" onClick={openRevisions}>Revisions</Button>
                <Button variant="outline" size="sm" onClick={()=>openImportWizard()}>Import wizard</Button>
                <Button variant="outline" size="sm" onClick={() => openDialog("variants")}>
                  <Layers /> Variants
                </Button>
              </div>
            </Section>
          </>
        )}
        <CollapsibleSection title="Open libraries" count={libs.length} dirty={libs.some(l=>l.dirty)} preferenceKey="pcbplannercad.sidebar.openLibrariesExpanded">
          {libs.map((l) => (
            <TreeRow
              key={l.id}
              icon={Icons.library}
              onClick={() => bus.emit("showLibrary", { libId: l.id })}
              title={l.path ?? ""}
              trailing={
                <span className="flex items-center gap-1.5">
                  {l.dirty && <span className="dirty-dot" />}
                  <span className="font-mono text-[10px] text-faint">{l.components}</span>
                </span>
              }
            >
              {l.name}
            </TreeRow>
          ))}
        </CollapsibleSection>
        {freeDocs.length > 0 && (
          <Section title="Free documents">
            {freeDocs.map((d) => (
              <TreeRow
                key={d.info.id}
                icon={d.info.kind === "pcb" ? Icons.board : Icons.schematic}
                active={d.info.id === active}
                onClick={() => bus.emit("showDoc", { docId: d.info.id })}
                trailing={d.info.dirty ? <span className="dirty-dot" /> : null}
              >
                {d.info.title}
              </TreeRow>
            ))}
          </Section>
        )}
      </div>
    </Panel>
  );
}
