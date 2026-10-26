import { useMemo, useState } from "react";
import { Cable, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { NativeSelect, NativeSelectOption } from "@/components/ui/native-select";
import { EmptyNote, Panel, PanelHeading } from "@/components/app/panel";
import { cn } from "@/lib/utils";
import { api } from "../core/api";
import { report } from "../core/actions";
import { bus } from "../core/bus";
import { activeDoc, useEditor, type DocState } from "../core/store";
import type { BoardItem } from "../core/types";
import { NM_PER_MM } from "../core/units";

interface NetRow {
  name: string;
  cls: string;
  pads: number;
  vias: number;
  /** Routed copper length (tracks + arcs), nm. */
  length: number;
  unrouted: number;
}

const natural = new Intl.Collator(undefined, { numeric: true, sensitivity: "base" });

/** Per-net statistics computed from the board items and the live ratsnest. */
function netRows(doc: DocState): { rows: NetRow[]; classes: string[] } {
  const board = [...doc.items.values()].find((i): i is BoardItem => i.kind === "board");
  const classes = board?.netClasses ?? [];
  const classOf = new Map<string, string>();
  for (const c of classes) for (const n of c.nets) classOf.set(n, c.name);
  const fallback = classes[0]?.name ?? "Default";
  const rows = new Map<string, NetRow>();
  const row = (name: string) => {
    let r = rows.get(name);
    if (!r) rows.set(name, (r = { name, cls: classOf.get(name) ?? fallback, pads: 0, vias: 0, length: 0, unrouted: 0 }));
    return r;
  };
  for (const it of doc.items.values()) {
    if (it.kind === "footprint") {
      for (const n of Object.values(it.padNets)) if (n) row(n).pads++;
    } else if (it.kind === "track" && it.net) row(it.net).length += Math.hypot(it.b.x - it.a.x, it.b.y - it.a.y);
    else if (it.kind === "arcTrack" && it.net) row(it.net).length += (Math.abs(it.sweep) * Math.PI * it.radius) / 180;
    else if (it.kind === "via" && it.net) row(it.net).vias++;
  }
  for (const w of doc.airwires) if (w.net) row(w.net).unrouted++;
  return { rows: [...rows.values()].sort((a, b) => natural.compare(a.name, b.name)), classes: classes.map((c) => c.name) };
}

export function PcbPanel() {
  const doc = useEditor((s) => (s.activeDocId ? s.docs[s.activeDocId] : undefined));
  const [filter, setFilter] = useState("");
  const [cls, setCls] = useState("");
  const [onlyOpen, setOnlyOpen] = useState(false);
  const [current, setCurrent] = useState<string | null>(null);
  const isBoard = doc?.info.kind === "pcb";
  const data = useMemo(() => (doc && isBoard ? netRows(doc) : { rows: [], classes: [] }), [doc?.items, doc?.airwires, isBoard]);

  if (!doc || !isBoard)
    return (
      <Panel>
        <PanelHeading title="PCB" />
        <EmptyNote icon={<Cable />} title="No board">
          Open a board to browse its nets, classes and routed lengths.
        </EmptyNote>
      </Panel>
    );

  const docId = doc.info.id;
  const f = filter.trim().toLowerCase();
  const rows = data.rows.filter((r) => (!f || r.name.toLowerCase().includes(f)) && (!cls || r.cls === cls) && (!onlyOpen || r.unrouted > 0));
  const open = data.rows.filter((r) => r.unrouted > 0).length;

  const highlight = async (net: string) => {
    setCurrent(net);
    try {
      useEditor.getState().patchDoc(docId, { marked: new Set(await api.pcbNetItems(docId, net)) });
    } catch (e) {
      report(e);
    }
  };
  const zoom = async (net: string) => {
    setCurrent(net);
    try {
      const ids = await api.pcbNetItems(docId, net);
      useEditor.getState().patchDoc(docId, { selection: new Set(ids), marked: new Set(ids) });
      if (activeDoc()?.info.id === docId) bus.emit("view", { action: "fitSelection" });
    } catch (e) {
      report(e);
    }
  };
  const clear = () => {
    setCurrent(null);
    useEditor.getState().patchDoc(docId, { marked: new Set() });
  };

  return (
    <Panel>
      <PanelHeading title="PCB · Nets">
        {current && (
          <Button variant="ghost" size="icon-sm" title="Clear highlight" onClick={clear}>
            <X />
          </Button>
        )}
      </PanelHeading>
      <div className="grid shrink-0 gap-1.5 border-b border-line p-2">
        <Input placeholder="Filter nets…" value={filter} onChange={(e) => setFilter(e.target.value)} />
        <div className="flex items-center gap-2">
          <NativeSelect value={cls} onChange={(e) => setCls(e.target.value)} wrapperClassName="flex-1">
            <NativeSelectOption value="">All net classes</NativeSelectOption>
            {data.classes.map((c) => (
              <NativeSelectOption key={c} value={c}>
                {c}
              </NativeSelectOption>
            ))}
          </NativeSelect>
          <label className="flex shrink-0 items-center gap-1.5 text-muted-foreground">
            <Checkbox className="size-3.5" checked={onlyOpen} onCheckedChange={(v) => setOnlyOpen(v === true)} /> Unrouted
          </label>
        </div>
      </div>
      <div className="grid shrink-0 grid-cols-[minmax(0,1fr)_34px_64px_30px] gap-2 border-b border-line bg-panel-head px-2 py-1 text-[10px] font-semibold text-faint">
        <span>Net</span>
        <span className="text-right">Pads</span>
        <span className="text-right">Length</span>
        <span className="text-right" title="Unrouted connections">Open</span>
      </div>
      <div className="min-h-0 flex-1 overflow-auto">
        {rows.map((r) => (
          <div
            key={r.name}
            onClick={() => void highlight(r.name)}
            onDoubleClick={() => void zoom(r.name)}
            title={`${r.name} · class ${r.cls} · ${r.vias} vias — click to highlight, double-click to zoom`}
            className={cn(
              "grid cursor-default grid-cols-[minmax(0,1fr)_34px_64px_30px] items-center gap-2 px-2 py-[3px] hover:bg-hover",
              current === r.name && "bg-sel hover:bg-sel",
            )}
          >
            <span className="min-w-0 truncate font-mono">
              {r.name}
              {r.cls !== data.classes[0] && <span className="ml-1.5 font-sans text-[10px] text-faint">{r.cls}</span>}
            </span>
            <span className="text-right font-mono text-muted-foreground">{r.pads}</span>
            <span className="text-right font-mono text-muted-foreground">{r.length ? (r.length / NM_PER_MM).toFixed(2) : "—"}</span>
            <span className={cn("text-right font-mono", r.unrouted ? "text-err-line" : "text-faint")}>{r.unrouted || "·"}</span>
          </div>
        ))}
        {!rows.length && <p className="px-3 py-3 text-muted-foreground">No nets match.</p>}
      </div>
      <div className="shrink-0 border-t border-line px-2 py-1 text-[10px] text-faint">
        {data.rows.length} nets · {open ? `${open} with unrouted connections` : "all routed"} · lengths in mm
      </div>
    </Panel>
  );
}
