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
import { activeDoc, useEditor } from "../core/store";
import { layer } from "../core/layers";
import { netRows } from "../pcb/boardTools";
import { NM_PER_MM } from "../core/units";

export function PcbPanel() {
  const doc = useEditor((s) => (s.activeDocId ? s.docs[s.activeDocId] : undefined));
  const [filter, setFilter] = useState("");
  const [cls, setCls] = useState("");
  const [onlyOpen, setOnlyOpen] = useState(false);
  const [current, setCurrent] = useState<string | null>(null);
  const isBoard = doc?.info.kind === "pcb";
  const layerNames = useMemo(() => {
    const m = new Map<number, string>();
    for (const it of doc?.items.values() ?? []) if (it.kind === "board") for (const l of it.stackup.layers) if (l.layer !== null) m.set(l.layer, l.name);
    return m;
  }, [doc?.items]);
  const data = useMemo(() => (doc && isBoard ? netRows(doc.items, doc.airwires) : { rows: [], classes: [] }), [doc?.items, doc?.airwires, isBoard]);

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
      <div className="grid shrink-0 grid-cols-[minmax(0,1fr)_30px_30px_58px_30px] gap-2 border-b border-line bg-panel-head px-2 py-1 text-[10px] font-semibold text-faint">
        <span>Net</span>
        <span className="text-right">Pads</span>
        <span className="text-right">Vias</span>
        <span className="text-right">Length</span>
        <span className="text-right" title="Unrouted connections">Open</span>
      </div>
      <div className="min-h-0 flex-1 overflow-auto">
        {rows.map((r) => (
          <div key={r.name}>
          <div
            onClick={() => void highlight(r.name)}
            onDoubleClick={() => void zoom(r.name)}
            title={`${r.name} · class ${r.cls} — click to highlight and show lengths per layer, double-click to zoom`}
            className={cn(
              "grid cursor-default grid-cols-[minmax(0,1fr)_30px_30px_58px_30px] items-center gap-2 px-2 py-[3px] hover:bg-hover",
              current === r.name && "bg-sel hover:bg-sel",
            )}
          >
            <span className="min-w-0 truncate font-mono">
              {r.name}
              {r.cls !== data.classes[0] && <span className="ml-1.5 font-sans text-[10px] text-faint">{r.cls}</span>}
            </span>
            <span className="text-right font-mono text-muted-foreground">{r.pads}</span>
            <span className="text-right font-mono text-muted-foreground">{r.vias || "·"}</span>
            <span className="text-right font-mono text-muted-foreground">{r.length ? (r.length / NM_PER_MM).toFixed(2) : "—"}</span>
            <span className={cn("text-right font-mono", r.unrouted ? "text-err-line" : "text-faint")}>{r.unrouted || "·"}</span>
          </div>
          {current === r.name && r.byLayer.size > 0 && (
            <div className="grid gap-0.5 border-b border-line/60 bg-sel/40 py-1 pl-5 pr-2 text-[11px]">
              {[...r.byLayer].sort((a, b) => a[0] - b[0]).map(([l, len]) => (
                <div key={l} className="flex justify-between gap-2">
                  <span className="text-muted-foreground">{layerNames.get(l) ?? layer(l).name}</span>
                  <span className="font-mono">{(len / NM_PER_MM).toFixed(2)} mm · {Math.round((100 * len) / r.length)}%</span>
                </div>
              ))}
              {r.vias > 0 && <div className="flex justify-between gap-2"><span className="text-muted-foreground">Layer changes</span><span className="font-mono">{r.vias} via{r.vias === 1 ? "" : "s"}</span></div>}
            </div>
          )}
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
