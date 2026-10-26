import type { ReactNode } from "react";
import { Kbd } from "@/components/ui/kbd";
import { Separator } from "@/components/ui/separator";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";
import { bus } from "../core/bus";
import { useEditor } from "../core/store";
import { Icons } from "../shell/icons";
import { TOOLS } from "./profiles";
import { activateTool } from "./toolActions";

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

/** Floating tool strip at the top of the editor. */
export function ActiveBar({ docId }: { docId: string }) {
  const kind = useEditor((s) => s.docs[docId]?.info.kind);
  const tool = useEditor((s) => s.docs[docId]?.tool);
  const singleLayer = useEditor((s) => s.docs[docId]?.singleLayer);
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
        <BarButton label="Single layer mode" keys="Shift+S" active={singleLayer} onClick={() => useEditor.getState().patchDoc(docId, { singleLayer: !singleLayer })}>
          {Icons.singleLayer}
        </BarButton>
      )}
    </div>
  );
}
