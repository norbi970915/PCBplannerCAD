import { useEffect, useState } from "react";
import { Minus, Plus } from "lucide-react";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import { api } from "../core/api";
import { report } from "../core/actions";
import { useEditor } from "../core/store";
import type { LibSymbol } from "../core/types";
import { PropertiesPanel } from "../panels/PropertiesPanel";
import { useSurfaceWindow } from "../shell/SurfaceWindow";
import { CanvasView } from "./CanvasView";

/** A schematic, PCB or footprint document tab. */
export function DocumentPanel({ docId }: { docId: string }) {
  const detached = useSurfaceWindow() !== window;
  const exists = useEditor((s) => !!s.docs[docId]);
  if (!exists) return <div className="grid h-full place-items-center bg-panel text-xs text-muted-foreground">This document is closed.</div>;
  if (!detached) return <CanvasView docId={docId} />;
  return <div className="flex h-full min-h-0" onPointerDownCapture={() => useEditor.getState().setActiveDoc(docId)}>
    <div className="min-w-0 flex-1"><CanvasView docId={docId} /></div>
    <aside aria-label="Document properties, view and DRC" className="w-72 shrink-0 border-l border-line"><PropertiesPanel docId={docId} /></aside>
  </div>;
}

const unitLetter = (i: number) => String.fromCharCode(65 + i);

/** Symbol editor: one canvas per unit with A/B/C tabs. */
export function SymbolEditorPanel({ libId, name }: { libId: string; name: string }) {
  const [units, setUnits] = useState<string[]>([]);
  const [current, setCurrent] = useState(0);
  const [symbol, setSymbol] = useState<LibSymbol | null>(null);
  const revision = useEditor((s) => s.libRevision[libId] ?? 0);
  const readonly = useEditor((s) => s.libs.find((l) => l.id === libId)?.readonly ?? false);

  useEffect(() => {
    let cancelled = false;
    api
      .openSymbol(libId, name)
      .then(async (infos) => {
        for (const info of infos) if (!useEditor.getState().docs[info.id]) useEditor.getState().loadDoc(await api.snapshot(info.id));
        if (!cancelled) setUnits(infos.map((i) => i.id));
      })
      .catch(report);
    return () => {
      cancelled = true;
    };
  }, [libId, name]);

  useEffect(() => {
    api
      .libGet(libId)
      .then((lib) => setSymbol(lib.symbols.find((s) => s.name === name) ?? null))
      .catch(report);
  }, [libId, name, revision]);

  const docId = units[Math.min(current, units.length - 1)];
  useEffect(() => {
    if (docId) useEditor.getState().setActiveDoc(docId);
  }, [docId]);

  const changeUnits = async (op: "add" | "remove") => {
    try {
      const infos = op === "add" ? await api.addSymbolUnit(libId, name) : await api.removeSymbolUnit(libId, name, current);
      for (const info of infos) useEditor.getState().loadDoc(await api.snapshot(info.id));
      setUnits(infos.map((i) => i.id));
      setCurrent(op === "add" ? infos.length - 1 : Math.max(0, current - 1));
      useEditor.getState().bumpLib(libId);
    } catch (e) {
      report(e);
    }
  };

  return (
    <div className="flex h-full flex-col">
      <div className="flex h-8 shrink-0 items-center gap-1.5 border-b border-line bg-panel-head px-2.5 text-xs">
        <span className="mr-2 font-mono font-semibold">{name}</span>
        <span className="eyebrow">Unit</span>
        {units.map((id, i) => (
          <button
            key={id}
            onClick={() => setCurrent(i)}
            className={cn(
              "h-5 min-w-6 rounded-[4px] border border-line bg-secondary px-1.5 font-mono text-[11px] text-muted-foreground hover:bg-[var(--ui-btn-hover)]",
              i === current && "border-accent-ink bg-sel text-accent-ink",
            )}
          >
            {unitLetter(i)}
          </button>
        ))}
        {!readonly && (
          <>
            <Button variant="ghost" size="icon-xs" title="Add unit" onClick={() => void changeUnits("add")}>
              <Plus />
            </Button>
            {units.length > 1 && (
              <Button variant="ghost" size="icon-xs" title="Remove this unit" onClick={() => void changeUnits("remove")}>
                <Minus />
              </Button>
            )}
          </>
        )}
        {readonly && <span className="ml-2 text-[11px] text-faint">Read-only library — use Save As to edit a copy</span>}
      </div>
      <div className="min-h-0 flex-1">{docId && <CanvasView key={docId} docId={docId} symbol={symbol} />}</div>
    </div>
  );
}
