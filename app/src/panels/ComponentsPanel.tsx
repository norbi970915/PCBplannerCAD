import { useEffect, useMemo, useState } from "react";
import { ChevronDown, ChevronRight, Download, FolderOpen, Search } from "lucide-react";
import { Button } from "@/components/ui/button";
import { InputGroup, InputGroupAddon, InputGroupInput } from "@/components/ui/input-group";
import { NativeSelect, NativeSelectOption } from "@/components/ui/native-select";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Panel, PanelHeading } from "@/components/app/panel";
import { cn } from "@/lib/utils";
import { api } from "../core/api";
import { importLibrary, openLibrary, report } from "../core/actions";
import { bus } from "../core/bus";
import { useEditor } from "../core/store";
import type { CachedPart, ComponentEntry, Footprint } from "../core/types";
import { PreviewCanvas } from "../editor/PreviewCanvas";
import { startPlacePart } from "../editor/toolActions";

const LIMIT = 300;
const basename = (p: string) => p.split(/[\\/]/).pop() ?? p;

export function ComponentsPanel() {
  const libs = useEditor((s) => s.libs);
  const revisions = useEditor((s) => s.libRevision);
  const picked = useEditor((s) => s.pickedComponent);
  const [index, setIndex] = useState<ComponentEntry[]>([]);
  const [query, setQuery] = useState("");
  const [libFilter, setLibFilter] = useState("all");
  const [showMore, setShowMore] = useState(false);

  const libKey = libs.map((l) => `${l.id}:${l.components}`).join(",");
  useEffect(() => {
    api.componentIndex().then(setIndex).catch(report);
  }, [libKey, revisions]);

  const results = useMemo(() => {
    const tokens = query.toLowerCase().split(/\s+/).filter(Boolean);
    return index.filter((c) => {
      if (libFilter !== "all" && c.library !== libFilter) return false;
      const hay = `${c.name} ${c.description} ${c.value} ${c.libraryName} ${c.footprints.join(" ")}`.toLowerCase();
      return tokens.every((t) => hay.includes(t));
    });
  }, [index, query, libFilter]);

  const selected = picked ? index.find((c) => c.library === picked.library && c.name === picked.component) : undefined;

  return (
    <Panel>
      <PanelHeading title="Components">
        <Button variant="ghost" size="icon-sm" title="Open library file…" onClick={() => void openLibrary()}>
          <FolderOpen />
        </Button>
      </PanelHeading>
      <div className="grid gap-1.5 border-b border-line p-2">
        <NativeSelect value={libFilter} onChange={(e) => setLibFilter(e.target.value)}>
          <NativeSelectOption value="all">All libraries ({index.length})</NativeSelectOption>
          {libs.map((l) => (
            <NativeSelectOption key={l.id} value={l.id}>
              {l.name} ({l.components})
            </NativeSelectOption>
          ))}
        </NativeSelect>
        <InputGroup className="h-7">
          <InputGroupAddon>
            <Search className="size-3.5" />
          </InputGroupAddon>
          <InputGroupInput className="text-xs" placeholder="Search name, value, footprint…" value={query} onChange={(e) => setQuery(e.target.value)} />
        </InputGroup>
      </div>
      <div className="min-h-[120px] flex-1 overflow-auto py-1">
        {results.slice(0, LIMIT).map((c) => (
          <div
            key={`${c.library}/${c.name}`}
            onClick={() => useEditor.getState().pickComponent({ library: c.library, component: c.name })}
            onDoubleClick={() => void startPlacePart(c.library, c.name)}
            title={c.description}
            className={cn("mx-1 flex h-6 cursor-default items-center justify-between gap-2 rounded-[4px] px-2 hover:bg-hover", selected === c && "bg-sel hover:bg-sel")}
          >
            <span className="truncate font-mono text-[11.5px]">{c.name}</span>
            <span className="shrink-0 text-[10.5px] text-faint">
              {c.libraryName}
              {c.units > 1 ? ` · ${c.units} units` : ""}
            </span>
          </div>
        ))}
        {results.length > LIMIT && <div className="px-3 py-2 text-muted-foreground">{results.length - LIMIT} more — refine the search</div>}
        {!results.length && <div className="px-3 py-2 text-muted-foreground">No matches.</div>}
      </div>
      {selected && <ComponentPreview entry={selected} />}
      <div className="border-t border-line">
        <button className="flex h-7 w-full items-center gap-1 px-2 text-muted-foreground hover:text-ink" onClick={() => setShowMore(!showMore)}>
          {showMore ? <ChevronDown className="size-3.5" /> : <ChevronRight className="size-3.5" />}
          <span className="eyebrow">Get more parts</span>
          <span className="ml-1 text-[11px]">structured format · XML design libraries</span>
        </button>
        {showMore && <LibraryBrowser />}
      </div>
    </Panel>
  );
}

function ComponentPreview({ entry }: { entry: ComponentEntry }) {
  const [part, setPart] = useState<CachedPart | null>(null);
  const [fp, setFp] = useState<Footprint | null>(null);
  const [unit, setUnit] = useState(0);
  const revision = useEditor((s) => s.libRevision[entry.library] ?? 0);

  useEffect(() => {
    setUnit(0);
    api.cachedPart(entry.library, entry.name).then(setPart).catch(() => setPart(null));
    if (entry.footprints[0]) api.findFootprint(entry.library, entry.footprints[0]).then(setFp).catch(() => setFp(null));
    else setFp(null);
  }, [entry, revision]);

  return (
    <div className="grid gap-1.5 border-t border-line bg-panel-head p-2">
      <div className="flex items-center justify-between gap-2">
        <div className="min-w-0">
          <div className="truncate font-mono text-xs font-semibold">{entry.name}</div>
          {entry.description && <div className="truncate text-[11px] text-muted-foreground">{entry.description}</div>}
        </div>
        <div className="flex shrink-0 gap-1">
          <Button variant="outline" size="sm" onClick={() => bus.emit("showLibrary", { libId: entry.library, select: { kind: "component", name: entry.name } })}>
            Library
          </Button>
          <Button size="sm" onClick={() => void startPlacePart(entry.library, entry.name)}>
            Place
          </Button>
        </div>
      </div>
      {part && part.symbol.units.length > 1 && (
        <Tabs value={String(unit)} onValueChange={(v) => setUnit(Number(v))}>
          <TabsList className="h-6">
            {part.symbol.units.map((_, i) => (
              <TabsTrigger key={i} value={String(i)} className="h-5 px-2 text-[11px]">
                {String.fromCharCode(65 + i)}
              </TabsTrigger>
            ))}
          </TabsList>
        </Tabs>
      )}
      <PreviewCanvas items={part?.symbol.units[unit]?.items ?? []} kind="symbol" symbol={part?.symbol} height={140} />
      <div className="truncate text-[11px] text-muted-foreground">{entry.footprints.length ? `Footprint · ${entry.footprints.join(", ")}` : "No footprint assigned"}</div>
      {fp && <PreviewCanvas items={fp.items} kind="footprint" height={110} />}
    </div>
  );
}

/** Lists the stock structured format libraries installed on this PC for one-click import. */
function LibraryBrowser() {
  const [paths, setPaths] = useState<{ symbols: string | null; footprints: string | null } | null>(null);
  const [files, setFiles] = useState<string[]>([]);
  const [kind, setKind] = useState<"symbols" | "footprints">("symbols");
  const [filter, setFilter] = useState("");

  useEffect(() => {
    api.libraryPaths().then(setPaths).catch(report);
  }, []);

  useEffect(() => {
    const dir = paths?.[kind];
    if (dir) api.listLibraryFiles(dir).then(setFiles).catch(report);
    else setFiles([]);
  }, [paths, kind]);

  const shown = files.filter((f) => basename(f).toLowerCase().includes(filter.toLowerCase()));

  return (
    <div className="grid gap-1.5 px-2 pb-2">
      <div className="flex flex-wrap gap-1">
        <Button variant="outline" size="sm" onClick={() => void importLibrary("structuredSymbols")}>
          <Download /> structured format symbols…
        </Button>
        <Button variant="outline" size="sm" onClick={() => void importLibrary("structuredFootprints")}>
          <Download /> structured format footprints…
        </Button>
        <Button variant="outline" size="sm" onClick={() => void importLibrary("xmlLibrary")}>
          <Download /> XML design / XML .lbr…
        </Button>
      </div>
      {paths && (paths.symbols || paths.footprints) ? (
        <>
          <Tabs value={kind} onValueChange={(v) => setKind(v as "symbols" | "footprints")}>
            <TabsList className="h-6 w-full">
              <TabsTrigger value="symbols" className="h-5 text-[11px]">
                Installed symbols
              </TabsTrigger>
              <TabsTrigger value="footprints" className="h-5 text-[11px]">
                Installed footprints
              </TabsTrigger>
            </TabsList>
          </Tabs>
          <InputGroup className="h-7">
            <InputGroupAddon>
              <Search className="size-3.5" />
            </InputGroupAddon>
            <InputGroupInput className="text-xs" placeholder="Filter structured format libraries…" value={filter} onChange={(e) => setFilter(e.target.value)} />
          </InputGroup>
          <div className="max-h-52 overflow-auto rounded-[5px] border border-line bg-field py-0.5">
            {shown.map((f) => (
              <div
                key={f}
                title={`Import ${f}`}
                onClick={() => void importLibrary(kind === "symbols" ? "structuredSymbols" : "structuredFootprints", f)}
                className="flex h-6 cursor-default items-center justify-between px-2 hover:bg-hover"
              >
                <span className="truncate font-mono text-[11px]">{basename(f).replace(/\.(kicad_sym|pretty)$/, "")}</span>
                <span className="text-[10px] text-accent-ink">import</span>
              </div>
            ))}
          </div>
        </>
      ) : (
        <p className="text-[11px] text-muted-foreground">No structured format installation found. Use the buttons above to import library files.</p>
      )}
    </div>
  );
}
