import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { create } from "zustand";
import { cn } from "@/lib/utils";
import { bus } from "../core/bus";
import { highlightNetOnBoard, repour } from "../core/pcbActions";
import { useEditor } from "../core/store";
import type { Item } from "../core/types";
import { COMMANDS, commandAvailable, runCommand } from "./commands";

type Entry = { label: string; keys?: string; run: () => unknown; disabled?: boolean } | "-";

interface MenuState {
  at: { x: number; y: number; docId: string } | null;
  open: (x: number, y: number, docId: string) => void;
  close: () => void;
}

export const useContextMenu = create<MenuState>()((set) => ({
  at: null,
  open: (x, y, docId) => set({ at: { x, y, docId } }),
  close: () => set({ at: null }),
}));

const st = () => useEditor.getState();

function cmd(id: string, label?: string): Entry {
  const c = COMMANDS[id];
  if (!c) return "-";
  return { label: label ?? c.label, keys: c.keys?.[0] ?? c.display, run: () => runCommand(id), disabled: !commandAvailable(id) };
}

const netOf = (item: Item | undefined): string => {
  if (!item) return "";
  if (item.kind === "track" || item.kind === "arcTrack" || item.kind === "via" || item.kind === "zone") return item.net;
  return "";
};

/** Context-dependent entries for the active selection of a document. */
function entriesFor(docId: string): Entry[] {
  const doc = st().docs[docId];
  if (!doc) return [];
  const sel = [...doc.selection];
  const one = sel.length === 1 ? doc.items.get(sel[0]) : undefined;
  const board = doc.info.kind === "pcb";
  const out: Entry[] = [];

  if (!sel.length) {
    out.push(cmd("edit.paste"), cmd("edit.selectAll"), "-", cmd("view.fit"));
    if (board) out.push(cmd("view.pourFills", st().showPourFills ? "Hide Pour Fills" : "Show Pour Fills"), "-", cmd("tools.repour"), cmd("tools.drc"));
    return out;
  }

  const net = netOf(one);
  if (board && net) out.push({ label: `Highlight Net "${net}"`, run: () => highlightNetOnBoard(docId, net) });
  if (board && one?.kind === "zone") {
    if (!one.keepout) out.push({ label: "Repour This Polygon", run: () => repour(docId, sel[0]) });
    out.push({ label: "Reshape Polygon", keys: "E R", run: () => st().patchDoc(docId, { tool: "reshape" }) });
    out.push(cmd("place.pcb.polygonCut"), cmd("place.pcb.polygonSlice"));
  }
  if (board && one?.kind === "footprint") {
    out.push(cmd("edit.mirror", "Flip to Other Side"), { label: "View in 3D", keys: "3", run: () => bus.emit("show3d", { docId }) });
  }
  if (out.length) out.push("-");
  out.push(cmd("edit.rotate"));
  if (!board) out.push(cmd("edit.mirror"));
  out.push("-", cmd("edit.cut"), cmd("edit.copy"), cmd("edit.paste"), cmd("edit.duplicate"), cmd("edit.delete"));
  out.push("-", cmd("view.fitSelection"), { label: "Properties…", run: () => bus.emit("showPanel", { id: "properties" }) });
  if (doc.marked.size) out.push({ label: "Clear Highlight", run: () => st().patchDoc(docId, { marked: new Set() }) });
  return out;
}

export function ContextMenu() {
  const at = useContextMenu((s) => s.at);
  const close = useContextMenu((s) => s.close);
  const [entries, setEntries] = useState<Entry[]>([]);
  const [index, setIndex] = useState(-1);
  const [pos, setPos] = useState({ x: 0, y: 0 });
  const ref = useRef<HTMLDivElement>(null);

  useLayoutEffect(() => {
    if (!at) return;
    setEntries(entriesFor(at.docId));
    setIndex(-1);
  }, [at]);

  useLayoutEffect(() => {
    if (!at || !ref.current) return;
    const { offsetWidth: w, offsetHeight: h } = ref.current;
    setPos({ x: Math.max(4, Math.min(at.x, window.innerWidth - w - 4)), y: Math.max(4, Math.min(at.y, window.innerHeight - h - 4)) });
  }, [at, entries]);

  useEffect(() => {
    if (!at) return;
    const usable = entries.map((e, i) => (e !== "-" && !e.disabled ? i : -1)).filter((i) => i >= 0);
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") close();
      else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        if (!usable.length) return;
        const k = usable.indexOf(index);
        const next = e.key === "ArrowDown" ? usable[(k + 1) % usable.length] : usable[(k - 1 + usable.length) % usable.length];
        setIndex(next ?? usable[0]);
      } else if (e.key === "Enter") {
        const entry = entries[index];
        if (entry && entry !== "-" && !entry.disabled) {
          close();
          void Promise.resolve(entry.run());
        }
      } else return;
      e.preventDefault();
      e.stopPropagation();
    };
    const onDown = (e: PointerEvent) => {
      if (ref.current?.contains(e.target as Node)) return;
      e.stopPropagation();
      e.preventDefault();
      close();
    };
    window.addEventListener("keydown", onKey, true);
    window.addEventListener("pointerdown", onDown, true);
    window.addEventListener("blur", close);
    return () => {
      window.removeEventListener("keydown", onKey, true);
      window.removeEventListener("pointerdown", onDown, true);
      window.removeEventListener("blur", close);
    };
  }, [at, entries, index, close]);

  if (!at) return null;
  return (
    <div
      ref={ref}
      role="menu"
      className="fixed z-50 min-w-52 rounded-[5px] border border-line bg-popover py-1 text-xs text-popover-foreground shadow-lg"
      style={{ left: pos.x, top: pos.y }}
      onContextMenu={(e) => e.preventDefault()}
    >
      {entries.map((e, i) =>
        e === "-" ? (
          <div key={i} className="my-1 h-px bg-line" />
        ) : (
          <button
            key={i}
            role="menuitem"
            disabled={e.disabled}
            onMouseEnter={() => !e.disabled && setIndex(i)}
            onClick={() => {
              close();
              void Promise.resolve(e.run());
            }}
            className={cn("flex w-full cursor-default items-center gap-4 px-2.5 py-[3px] text-left", e.disabled && "opacity-40", i === index && "bg-sel text-accent-ink")}
          >
            <span className="truncate">{e.label}</span>
            {e.keys && <span className="ml-auto font-mono text-[10px] text-faint">{e.keys}</span>}
          </button>
        ),
      )}
    </div>
  );
}
