import { cn } from "@/lib/utils";
import { useMemo } from "react";
import { Filter } from "lucide-react";
import { Checkbox } from "@/components/ui/checkbox";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { FILTER_KINDS, filterActive, useSelectFilter } from "../core/selectFilter";
import { boardTabs, FOOTPRINT_TABS, layer } from "../core/layers";
import { useEditor } from "../core/store";

/** Layer tabs along the bottom of board editors. */
export function LayerTabs({ docId, tabs: override }: { docId: string; tabs?: number[] }) {
  const doc = useEditor((s) => s.docs[docId]);
  const fills = useEditor((s) => s.showPourFills);
  const filter = useSelectFilter();
  const items = doc?.items;
  const kind = doc?.info.kind;
  const tabs = useMemo(
    () => (override ? override.map((id) => ({ id, name: layer(id).name })) : kind === "footprint" ? FOOTPRINT_TABS.map((id) => ({ id, name: layer(id).name })) : boardTabs(items?.values() ?? [])),
    [override, kind, items],
  );
  if (!doc) return null;
  const current = layer(doc.activeLayer);
  const patch = useEditor.getState().patchDoc;

  return (
    <div className="flex h-[26px] shrink-0 items-stretch overflow-x-auto border-t border-line bg-chrome [scrollbar-width:none]">
      <span className="w-6 shrink-0 border-r border-line" style={{ background: current.color }} title={`Current layer: ${current.name}`} />
      <button
        className={cn("shrink-0 border-r border-line px-2 font-mono text-[10px] font-semibold text-faint hover:bg-hover", doc.singleLayer && "bg-sel text-accent-ink")}
        onClick={() => patch(docId, { singleLayer: !doc.singleLayer })}
        title="Single layer mode (Shift+S)"
      >
        LS
      </button>
      {kind === "pcb" && (
        <button
          className={cn("shrink-0 border-r border-line px-2 text-[10px] font-semibold text-faint hover:bg-hover", fills && "bg-sel text-accent-ink")}
          onClick={() => useEditor.getState().togglePourFills()}
          title={fills ? "Pour fills shown — click for outlines only (V P)" : "Pours shown as outlines — click to show fills (V P)"}
        >
          Fills
        </button>
      )}
      {tabs.map(({ id, name }) => {
        const l = { ...layer(id), name };
        const active = id === doc.activeLayer;
        return (
          <button
            key={id}
            onClick={() => patch(docId, { activeLayer: id })}
            title={`${l.name} (+/- to cycle)`}
            className={cn(
              "flex shrink-0 items-center gap-1.5 border-r border-line px-3 text-[11px] text-muted-foreground hover:bg-hover hover:text-ink",
              active && "bg-panel text-ink shadow-[inset_0_-2px_0_var(--ui-accent)]",
            )}
          >
            <span className="swatch" style={{ background: l.color }} />
            {l.name}
          </button>
        );
      })}
      {kind === "pcb" && (
        <Popover>
          <PopoverTrigger asChild>
            <button
              className={cn(
                "sticky right-0 ml-auto flex shrink-0 items-center gap-1 border-l border-line bg-chrome px-2.5 text-[11px] text-muted-foreground hover:bg-hover hover:text-ink",
                filterActive(filter.on) && "bg-sel text-accent-ink",
              )}
              title="Selection filter: which objects clicks and box selection pick"
            >
              <Filter className="size-3" /> {filterActive(filter.on) ? "Filtered" : "Filter"}
            </button>
          </PopoverTrigger>
          <PopoverContent side="top" align="end" className="w-56 p-2 text-xs">
            <div className="mb-1.5 flex items-center justify-between">
              <span className="eyebrow">Selectable objects</span>
              <button className="text-[11px] text-accent-ink hover:underline" onClick={filter.all}>
                All
              </button>
            </div>
            {FILTER_KINDS.map(([k, label]) => (
              <label key={k} className="flex items-center gap-2 rounded-[4px] px-1 py-1 hover:bg-hover">
                <Checkbox className="size-3.5" checked={filter.on[k]} onCheckedChange={(v) => filter.set(k, v === true)} />
                {label}
              </label>
            ))}
          </PopoverContent>
        </Popover>
      )}
    </div>
  );
}
