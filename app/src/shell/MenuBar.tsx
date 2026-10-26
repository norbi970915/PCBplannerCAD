import { Moon, Sun } from "lucide-react";
import {
  Menubar,
  MenubarContent,
  MenubarItem,
  MenubarMenu,
  MenubarSeparator,
  MenubarShortcut,
  MenubarSub,
  MenubarSubContent,
  MenubarSubTrigger,
  MenubarTrigger,
} from "@/components/ui/menubar";
import { Button } from "@/components/ui/button";
import { useEditor } from "../core/store";
import { useTheme } from "../core/theme";
import { TOOLS } from "../editor/profiles";
import { COMMANDS, commandAvailable, runCommand, type CommandId } from "./commands";

type MenuEntry = CommandId | "-" | { title: string; items: MenuEntry[] };

function menus(kind: string | undefined): { title: string; items: MenuEntry[] }[] {
  const place: MenuEntry[] = kind ? TOOLS[kind as keyof typeof TOOLS].filter((t) => t.id !== "select").map((t) => `place.${kind}.${t.id}`) : [];
  return [
    {
      title: "File",
      items: [
        "file.newProject",
        "file.openProject",
        "file.closeProject",
        "-",
        "file.newSchematic",
        "file.newPcb",
        "file.newLibrary",
        "-",
        "file.open",
        "file.save",
        "file.saveAs",
        "file.saveAll",
        "file.close",
        "-",
        "file.outputs",
        "file.print",
        "file.gerberViewer",
        "-",
        {
          title: "Import",
          items: ["import.structuredProject", "import.structuredSchematic", "import.structuredSymbols", "import.structuredFootprints", "-", "import.xmlSchematic", "import.xmlLibrary"],
        },
        "-",
        "file.exit",
      ],
    },
    {
      title: "Edit",
      items: ["edit.undo", "edit.redo", "-", "edit.cut", "edit.copy", "edit.paste", "edit.duplicate", "edit.delete", "-", "edit.rotate", "edit.mirror", "-", "edit.selectAll", "edit.deselectAll"],
    },
    {
      title: "View",
      items: ["view.fit", "view.fitSelection", "view.zoomIn", "view.zoomOut", "-", "view.toggleUnits", "view.gridNext", "view.gridPrev", "-", "view.singleLayer", "view.pourFills", "view.layerNext", "view.layerPrev", "-", "view.3d", "view.theme"],
    },
    { title: "Place", items: place },
    { title: "Design", items: ["design.updatePcb", "-", "design.layerStack", "design.rules", "-", "design.impedance"] },
    { title: "Project", items: ["project.compile", "-", "project.annotate", "project.annotateAll", "-", "project.bom", "project.variants", "-", "project.addSheet", "project.board"] },
    { title: "Tools", items: ["tools.drc", "tools.measure", "tools.repour", "tools.tune", "tools.clearMarks", "-", "tools.footprintWizard", "tools.symbolWizard", "-", "tools.panelize", "tools.scripts"] },
    { title: "Window", items: ["window.projects", "window.components", "window.pcb", "window.properties", "window.messages", "-", "window.resetLayout"] },
    { title: "Help", items: ["help.about"] },
  ];
}

function Entries({ items }: { items: MenuEntry[] }) {
  if (!items.length) return <MenubarItem disabled>Open a document first</MenubarItem>;
  return (
    <>
      {items.map((entry, j) => {
        if (entry === "-") return <MenubarSeparator key={j} />;
        if (typeof entry === "object") {
          return (
            <MenubarSub key={entry.title}>
              <MenubarSubTrigger>{entry.title}</MenubarSubTrigger>
              <MenubarSubContent className="min-w-64">
                <Entries items={entry.items} />
              </MenubarSubContent>
            </MenubarSub>
          );
        }
        const cmd = COMMANDS[entry];
        if (!cmd) return null;
        const keys = cmd.display ?? cmd.keys?.join(", ");
        return (
          <MenubarItem key={entry} disabled={!commandAvailable(entry)} onSelect={() => runCommand(entry)}>
            {cmd.label}
            {keys && <MenubarShortcut className="font-mono text-[10px]">{keys}</MenubarShortcut>}
          </MenubarItem>
        );
      })}
    </>
  );
}

export function MenuBar() {
  const kind = useEditor((s) => (s.activeDocId ? s.docs[s.activeDocId]?.info.kind : undefined));
  const project = useEditor((s) => s.project?.name);
  // Re-render on document changes so enabled states stay current.
  useEditor((s) => s.docs);
  const resolved = useTheme((s) => s.resolved);
  const toggle = useTheme((s) => s.toggle);

  return (
    <div className="flex h-9 shrink-0 items-center gap-1 border-b border-line bg-chrome px-2">
      <span className="mr-2 pl-1 text-[13px] font-semibold tracking-tight text-ink">
        PCBPlannerCAD<span className="title-dot">·</span>
      </span>
      <Menubar className="h-8">
        {menus(kind).map((menu) => (
          <MenubarMenu key={menu.title}>
            <MenubarTrigger className="h-7 px-2.5 text-xs font-normal text-muted-foreground data-[state=open]:text-foreground">{menu.title}</MenubarTrigger>
            <MenubarContent className="min-w-64">
              <Entries items={menu.items} />
            </MenubarContent>
          </MenubarMenu>
        ))}
      </Menubar>
      <div className="ml-auto flex items-center gap-2">
        {project && <span className="text-[11px] text-faint">Project · <span className="text-muted-foreground">{project}</span></span>}
        <Button variant="ghost" size="icon-sm" onClick={toggle} title={resolved === "dark" ? "Switch to light theme" : "Switch to dark theme"}>
          {resolved === "dark" ? <Sun /> : <Moon />}
        </Button>
      </div>
    </div>
  );
}
