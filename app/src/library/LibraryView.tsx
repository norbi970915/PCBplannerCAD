import { useEffect, useMemo, useState } from "react";
import { Copy, FolderPlus, Pencil, Plus, Save, Search, Trash2, Wand2 } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { InputGroup, InputGroupAddon, InputGroupInput } from "@/components/ui/input-group";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { EmptyNote } from "@/components/app/panel";
import { cn } from "@/lib/utils";
import { api, type EntryKind } from "../core/api";
import {
  addLibraryToProject,
  closeLibrary,
  openFootprintEditor,
  openSymbolEditor,
  report,
  refreshLibs,
  saveLibrary,
} from "../core/actions";
import { useEditor } from "../core/store";
import type { Component, Footprint, Library } from "../core/types";
import { PreviewCanvas } from "../editor/PreviewCanvas";
import { startPlacePart } from "../editor/toolActions";
import { Icons } from "../shell/icons";
import { promptFields, useModal } from "../shell/modal";
import { ComponentForm } from "./ComponentForm";
import { FootprintWizard } from "./FootprintWizard";
import { SymbolWizard } from "./SymbolWizard";

type Tab = "component" | "symbol" | "footprint";
const LIST_LIMIT = 500;

export function useLibrary(libId: string): Library | null {
  const revision = useEditor((s) => s.libRevision[libId] ?? 0);
  const [lib, setLib] = useState<Library | null>(null);
  useEffect(() => {
    let cancelled = false;
    api
      .libGet(libId)
      .then((l) => !cancelled && setLib(l))
      .catch(report);
    return () => {
      cancelled = true;
    };
  }, [libId, revision]);
  return lib;
}

export function LibraryView({ libId }: { libId: string }) {
  const info = useEditor((s) => s.libs.find((l) => l.id === libId));
  const hasProject = useEditor((s) => !!s.project);
  const lib = useLibrary(libId);
  const [tab, setTab] = useState<Tab>("component");
  const [filter, setFilter] = useState("");
  const [selected, setSelected] = useState<string | null>(null);
  const readonly = info?.readonly ?? false;

  const names = useMemo(() => {
    if (!lib) return [];
    const list: { name: string; description: string }[] =
      tab === "component" ? lib.components : tab === "symbol" ? lib.symbols : lib.footprints;
    const f = filter.trim().toLowerCase();
    return list.filter((e) => !f || e.name.toLowerCase().includes(f) || e.description.toLowerCase().includes(f));
  }, [lib, tab, filter]);

  useEffect(() => {
    if (selected && !names.some((n) => n.name === selected)) setSelected(null);
  }, [names, selected]);

  const changed = async () => {
    useEditor.getState().bumpLib(libId);
    await refreshLibs();
  };

  const createEntry = async () => {
    try {
      if (tab === "symbol") {
        const r = await promptFields("New symbol", [
          { name: "name", label: "Name", value: "NewSymbol" },
          { name: "prefix", label: "Designator prefix", value: "U" },
        ]);
        if (!r?.name.trim()) return;
        const name = await api.newSymbol(libId, r.name.trim(), r.prefix.trim() || "U");
        await changed();
        setSelected(name);
        void openSymbolEditor(libId, name);
      } else if (tab === "footprint") {
        const r = await promptFields("New footprint", [{ name: "name", label: "Name", value: "NewFootprint" }]);
        if (!r?.name.trim()) return;
        const name = await api.newFootprint(libId, r.name.trim());
        await changed();
        setSelected(name);
        void openFootprintEditor(libId, name);
      } else {
        const r = await promptFields("New component", [
          { name: "name", label: "Name", value: "NewComponent" },
          { name: "symbol", label: "Symbol", value: lib?.symbols[0]?.name ?? "" },
        ]);
        if (!r?.name.trim()) return;
        const c: Component = { name: r.name.trim(), description: "", symbol: r.symbol.trim(), footprints: [], pinMap: [], parameters: [{ name: "Value", value: r.name.trim() }] };
        await api.putComponent(libId, c, null);
        await changed();
        setSelected(c.name);
      }
    } catch (e) {
      report(e);
    }
  };

  const duplicate = async () => {
    if (!selected) return;
    try {
      const n = await api.duplicateEntry(libId, tab as EntryKind, selected);
      await changed();
      setSelected(n);
    } catch (e) {
      report(e);
    }
  };

  const remove = async () => {
    if (!selected) return;
    try {
      const closed = await api.deleteEntry(libId, tab as EntryKind, selected);
      for (const id of closed) useEditor.getState().removeDoc(id);
      await changed();
      setSelected(null);
    } catch (e) {
      report(e);
    }
  };

  const edit = (name: string) => {
    if (tab === "symbol") void openSymbolEditor(libId, name);
    else if (tab === "footprint") void openFootprintEditor(libId, name);
  };

  if (!lib || !info) return <div className="grid h-full place-items-center bg-panel text-xs text-muted-foreground">Loading library…</div>;

  const counts = { component: lib.components.length, symbol: lib.symbols.length, footprint: lib.footprints.length };
  return (
    <div className="flex h-full flex-col bg-panel text-xs">
      <div className="flex h-10 shrink-0 items-center justify-between gap-3 border-b border-line bg-panel-head px-3">
        <div className="flex min-w-0 items-center gap-2">
          <span className="text-accent-ink">{Icons.library}</span>
          <span className="text-[13px] font-semibold">{lib.name}</span>
          {info.dirty && <span className="dirty-dot" title="Unsaved changes" />}
          {info.readonly && <Badge variant="secondary">read-only</Badge>}
          <span className="truncate font-mono text-[11px] text-faint">{info.readonly ? "built-in" : (info.path ?? "not saved yet")}</span>
        </div>
        <div className="flex shrink-0 gap-1.5">
          {!readonly && (
            <Button variant="outline" onClick={() => void saveLibrary(libId)}>
              <Save /> Save
            </Button>
          )}
          <Button variant="outline" onClick={() => void saveLibrary(libId, true)}>
            Save As…
          </Button>
          {hasProject && !info.inProject && !readonly && (
            <Button variant="outline" onClick={() => void addLibraryToProject(libId)}>
              <FolderPlus /> Add to Project
            </Button>
          )}
          {!readonly && (
            <Button variant="ghost" onClick={() => void closeLibrary(libId)}>
              Close
            </Button>
          )}
        </div>
      </div>
      <div className="grid min-h-0 flex-1 grid-cols-[320px_minmax(0,1fr)]">
        <div className="flex min-h-0 flex-col gap-2 border-r border-line p-2.5">
          <Tabs value={tab} onValueChange={(v) => (setTab(v as Tab), setSelected(null))}>
            <TabsList className="w-full">
              {(["component", "symbol", "footprint"] as Tab[]).map((t) => (
                <TabsTrigger key={t} value={t} className="text-[11px]">
                  {t === "component" ? "Components" : t === "symbol" ? "Symbols" : "Footprints"}
                  <span className="font-mono text-[10px] text-faint">{counts[t]}</span>
                </TabsTrigger>
              ))}
            </TabsList>
          </Tabs>
          <InputGroup>
            <InputGroupAddon>
              <Search className="size-3.5" />
            </InputGroupAddon>
            <InputGroupInput className="text-xs" placeholder="Filter by name or description…" value={filter} onChange={(e) => setFilter(e.target.value)} />
          </InputGroup>
          <div className="min-h-0 flex-1 overflow-auto rounded-[5px] border border-line bg-field py-0.5">
            {names.slice(0, LIST_LIMIT).map((e) => (
              <div
                key={e.name}
                onClick={() => setSelected(e.name)}
                onDoubleClick={() => edit(e.name)}
                title={e.description}
                className={cn("mx-0.5 flex cursor-default flex-col rounded-[4px] px-2 py-1 hover:bg-hover", selected === e.name && "bg-sel hover:bg-sel")}
              >
                <span className="truncate font-mono text-[11.5px]">{e.name}</span>
                {e.description && <span className="truncate text-[10.5px] text-faint">{e.description}</span>}
              </div>
            ))}
            {names.length > LIST_LIMIT && <div className="px-3 py-2 text-muted-foreground">{names.length - LIST_LIMIT} more — refine the filter</div>}
            {!names.length && <div className="px-3 py-2 text-muted-foreground">Nothing here yet.</div>}
          </div>
          {!readonly && (
            <div className="flex flex-wrap gap-1">
              <Button variant="outline" size="sm" onClick={() => void createEntry()}>
                <Plus /> New
              </Button>
              <Button variant="outline" size="sm" disabled={!selected} onClick={() => void duplicate()}>
                <Copy /> Duplicate
              </Button>
              <Button variant="outline" size="sm" disabled={!selected} onClick={() => void remove()}>
                <Trash2 /> Delete
              </Button>
              {tab === "footprint" && (
                <Button size="sm" onClick={() => useModal.getState().show(<FootprintWizard libId={libId} onDone={(n) => (void changed(), setSelected(n))} />)}>
                  <Wand2 /> Footprint wizard
                </Button>
              )}
              {tab === "symbol" && (
                <Button size="sm" onClick={() => useModal.getState().show(<SymbolWizard libId={libId} onDone={(n) => (void changed(), setSelected(n))} />)}>
                  <Wand2 /> Symbol wizard
                </Button>
              )}
            </div>
          )}
        </div>
        <div className="min-w-0 overflow-auto p-4">
          {selected && tab === "component" && (
            <ComponentForm key={selected} libId={libId} lib={lib} name={selected} readonly={readonly} onSaved={(n) => (void changed(), setSelected(n))} onPlace={() => void startPlacePart(libId, selected)} />
          )}
          {selected && tab === "symbol" && <SymbolDetail lib={lib} name={selected} onEdit={() => edit(selected)} />}
          {selected && tab === "footprint" && <FootprintDetail fp={lib.footprints.find((f) => f.name === selected)} onEdit={() => edit(selected)} />}
          {!selected && (
            <EmptyNote icon={Icons.library} title="Select an entry">
              Pick a component, symbol or footprint on the left. Double-click a symbol or footprint to open its editor.
            </EmptyNote>
          )}
        </div>
      </div>
    </div>
  );
}

function DetailHead({ title, onEdit, label }: { title: string; onEdit: () => void; label: string }) {
  return (
    <div className="mb-2 flex items-center justify-between gap-3">
      <h3 className="font-mono text-[15px] font-semibold">{title}</h3>
      <Button onClick={onEdit}>
        <Pencil /> {label}
      </Button>
    </div>
  );
}

function Facts({ rows }: { rows: [string, React.ReactNode][] }) {
  return (
    <div className="my-2 grid grid-cols-[auto_1fr] gap-x-4 gap-y-1">
      {rows.map(([k, v]) => [
        <span key={k + "k"} className="text-muted-foreground">
          {k}
        </span>,
        <span key={k + "v"} className="truncate font-mono">
          {v}
        </span>,
      ])}
    </div>
  );
}

function SymbolDetail({ lib, name, onEdit }: { lib: Library; name: string; onEdit: () => void }) {
  const sym = lib.symbols.find((s) => s.name === name);
  const [unit, setUnit] = useState(0);
  if (!sym) return null;
  const pins = sym.units.flatMap((u) => u.items.filter((i) => i.kind === "pin")).length;
  return (
    <div>
      <DetailHead title={sym.name} onEdit={onEdit} label="Edit symbol" />
      <p className="text-muted-foreground">{sym.description || "No description."}</p>
      <Facts rows={[["Prefix", sym.prefix], ["Units", sym.units.length], ["Pins", pins]]} />
      {sym.units.length > 1 && (
        <Tabs value={String(unit)} onValueChange={(v) => setUnit(Number(v))} className="mb-2">
          <TabsList>
            {sym.units.map((_, i) => (
              <TabsTrigger key={i} value={String(i)} className="px-3 font-mono">
                {String.fromCharCode(65 + i)}
              </TabsTrigger>
            ))}
          </TabsList>
        </Tabs>
      )}
      <PreviewCanvas items={sym.units[unit]?.items ?? []} kind="symbol" symbol={sym} height={340} />
    </div>
  );
}

function FootprintDetail({ fp, onEdit }: { fp?: Footprint; onEdit: () => void }) {
  if (!fp) return null;
  const pads = fp.items.filter((i) => i.kind === "pad").length;
  return (
    <div>
      <DetailHead title={fp.name} onEdit={onEdit} label="Edit footprint" />
      <p className="text-muted-foreground">{fp.description || "No description."}</p>
      <Facts rows={[["Pads", pads], ["3D model", fp.model?.path ?? "—"]]} />
      <PreviewCanvas items={fp.items} kind="footprint" height={340} />
    </div>
  );
}
