import type { ReactNode } from "react";
import { Funnel, ShieldCheck, ShieldOff } from "lucide-react";
import { Kbd } from "@/components/ui/kbd";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { Separator } from "@/components/ui/separator";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";
import { bus } from "../core/bus";
import { filterActive, useSelectFilter } from "../core/selectFilter";
import { useEditor } from "../core/store";
import { SelectionFilter } from "../panels/SelectionFilter";
import { Icons } from "../shell/icons";
import { TOOLS } from "./profiles";
import { LAYER_MODES } from "../core/view";
import { activateTool, cycleLayerMode } from "./toolActions";

function BarButton({ label, keys, active, onClick, children }: { label: string; keys?: string; active?: boolean; onClick: () => void; children: ReactNode }) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <button
          aria-label={label}
          onClick={onClick}
          className={cn(
            "grid size-7 place-items-center rounded-[4px] text-muted-foreground transition-colors hover:bg-hover hover:text-ink",
            active && "bg-sel text-accent-ink shadow-[inset_0_0_0_1px_var(--ui-accent)] hover:bg-sel hover:text-accent-ink",
          )}
        >
          {children}
        </button>
      </TooltipTrigger>
      <TooltipContent side="bottom" className="flex items-center gap-2">
        {label}
        {keys && (
          <span className="flex gap-0.5">
            {keys.split(" ").map((k, i) => (
              <Kbd key={i}>{k}</Kbd>
            ))}
          </span>
        )}
      </TooltipContent>
    </Tooltip>
  );
}

/** Keeps editing modes and the next action visible while cutting pours. */
export function PolygonEditBar({ docId }: { docId: string }) {
  const tool = useEditor(s => s.docs[docId]?.tool);
  const hint = useEditor(s => s.hint);
  if (!["polygonCut", "polygonSlice", "reshape"].includes(tool ?? "")) return null;
  return (
    <div className="absolute top-12 left-3 right-3 flex w-fit max-w-[calc(100%-24px)] flex-col gap-1 rounded-[5px] border border-line-strong bg-card/95 px-2 py-1.5 text-xs shadow-md" onPointerDown={e => e.stopPropagation()}>
      <div className="flex items-center gap-1" role="toolbar" aria-label="Polygon editing">
        {([['polygonCut', 'Cut', 'E C'], ['polygonSlice', 'Slice', 'E S'], ['reshape', 'Reshape', 'E R']] as const).map(([id, label, keys]) => (
          <button key={id} aria-pressed={tool === id} title={`${label} (${keys})`} className={cn("flex h-6 items-center gap-1.5 rounded-[4px] px-2 hover:bg-hover", tool === id && "bg-sel text-accent-ink")} onClick={() => activateTool(id, docId)}>
            {Icons[id === "reshape" ? "reshape" : id]}{label}
          </button>
        ))}
        <span className="mx-1 h-4 border-l border-line" />
        <button className="h-6 rounded-[4px] px-2 text-muted-foreground hover:bg-hover" onClick={() => activateTool("select", docId)}>Done <span className="font-mono text-[10px]">Esc</span></button>
      </div>
      <div className="max-w-[760px] text-[11px] text-muted-foreground" aria-live="polite">{hint}</div>
    </div>
  );
}

/** A visible way out of routing, so pad clicks are not mistaken for selection. */
export function RoutingBar({ docId }: { docId: string }) {
  const tool = useEditor(s => s.docs[docId]?.tool);
  const hint = useEditor(s => s.hint);
  if (tool !== "route" && tool !== "diffPair") return null;
  return <div className="absolute top-12 left-3 right-3 flex w-fit max-w-[calc(100%-24px)] items-start gap-3 rounded border border-line-strong bg-card/95 px-3 py-2 text-xs shadow-md" onPointerDown={e => e.stopPropagation()}>
    <div><div className="font-semibold text-ink">Interactive routing</div><div className="max-w-[720px] pt-1 text-[11px] text-muted-foreground" aria-live="polite">{hint}</div></div>
    <button className="shrink-0 rounded bg-sel px-2 py-1 text-accent-ink hover:bg-hover" onClick={() => activateTool("select", docId)}>Select / move <span className="font-mono">Esc</span></button>
  </div>;
}

/** Design rules on (walk around / push) or off (ignore violators). */
function RulesButton() {
  const on = useEditor((s) => s.drcEnforce);
  return (
    <BarButton
      label={on ? "Design rules ON: routing and moves keep clearances" : "Design rules OFF: routing and moves ignore violators"}
      keys="Ctrl+Shift+D"
      active={!on}
      onClick={() => useEditor.getState().toggleDrcEnforce()}
    >
      {on ? <ShieldCheck className="size-4" /> : <ShieldOff className="size-4 text-err-line" />}
    </BarButton>
  );
}

/** Toolbar dropdown with the board selection filter; lit while anything is filtered out. */
function SelectionFilterButton({ docId }: { docId: string }) {
  const doc = useEditor((s) => s.docs[docId]);
  const active = useSelectFilter(filterActive);
  if (!doc) return null;
  return (
    <Popover>
      <Tooltip>
        <TooltipTrigger asChild>
          <PopoverTrigger asChild>
            <button
              aria-label="Selection filter"
              className={cn(
                "relative grid size-7 place-items-center rounded-[4px] text-muted-foreground transition-colors hover:bg-hover hover:text-ink",
                active && "text-accent-ink",
              )}
            >
              <Funnel className="size-4" />
              {active && <span className="absolute top-1 right-1 size-1.5 rounded-full bg-[var(--ui-accent)]" />}
            </button>
          </PopoverTrigger>
        </TooltipTrigger>
        <TooltipContent side="bottom">{active ? "Selection filter (some objects excluded)" : "Selection filter"}</TooltipContent>
      </Tooltip>
      <PopoverContent align="end" className="max-h-[70vh] w-64 overflow-y-auto p-2.5" onPointerDown={(e) => e.stopPropagation()}>
        <div className="eyebrow pb-1">Selection filter</div>
        <SelectionFilter doc={doc} />
      </PopoverContent>
    </Popover>
  );
}

/** Floating tool strip at the top of the editor. */
export function ActiveBar({ docId }: { docId: string }) {
  const kind = useEditor((s) => s.docs[docId]?.info.kind);
  const tool = useEditor((s) => s.docs[docId]?.tool);
  const layerMode = useEditor((s) => s.docs[docId]?.layerMode);
  if (!kind) return null;
  const boardLike = kind === "pcb" || kind === "footprint";

  return (
    <div
      className="absolute top-2 left-1/2 flex -translate-x-1/2 items-center gap-0.5 rounded-[5px] border border-line-strong bg-card/95 p-0.5 shadow-[0_4px_14px_#0000001f] backdrop-blur-sm"
      onPointerDown={(e) => e.stopPropagation()}
    >
      {TOOLS[kind]
        .filter((t) => !t.menuOnly)
        .map((t) => (
          <BarButton
            key={t.id}
            label={t.label}
            keys={t.keys}
            active={tool === t.id}
            onClick={() => {
              useEditor.getState().setActiveDoc(docId);
              activateTool(t.id, docId);
            }}
          >
            {Icons[t.icon]}
          </BarButton>
        ))}
      <Separator orientation="vertical" className="mx-1 h-4" />
      <BarButton label="Fit all" keys="V F" onClick={() => bus.emit("view", { action: "fit" })}>
        {Icons.fit}
      </BarButton>
      {boardLike && (
        <BarButton label={`Layer display: ${LAYER_MODES[layerMode ?? "all"].label}`} keys="Shift+S" active={layerMode !== "all"} onClick={() => cycleLayerMode(docId)}>
          {Icons.singleLayer}
        </BarButton>
      )}
      {kind === "pcb" && <SelectionFilterButton docId={docId} />}
      {kind === "pcb" && <RulesButton />}
    </div>
  );
}
