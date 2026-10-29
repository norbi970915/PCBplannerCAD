import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import {createPortal} from "react-dom";
import { cn } from "@/lib/utils";
import { activeDoc, useEditor } from "../core/store";
import { lastPointer, runFromMenu, sequenceEntries, setPending } from "./shortcuts";

const TITLES: Record<string, string> = {
  P: "Place",
  T: "Tools",
  V: "View",
  D: "Design",
  E: "Edit",
  F: "File",
  S: "Select",
  C: "Project",
  X: "Deselect",
  R: "Reports",
};

/**
 * Keyboard sequence menu: pressing the first key of a sequence (P, T,
 * V…) opens the commands it leads to at the pointer; the second key, a click
 * or Enter runs one.
 */
export function KeyMenu() {
  const prefix = useEditor((s) => s.pendingKeys);
  const kind = useEditor((s) => (s.activeDocId ? s.docs[s.activeDocId]?.info.kind : undefined));
  const entries = useMemo(() => (prefix ? sequenceEntries(prefix) : []), [prefix, kind]);
  const [index, setIndex] = useState(0);
  const [pos, setPos] = useState({ x: 0, y: 0 });
  const ref = useRef<HTMLDivElement>(null);
  const surface=lastPointer.window??window;

  // Open at the pointer, kept inside the window.
  useLayoutEffect(() => {
    if (!prefix) return;
    setIndex(Math.max(0, entries.findIndex((e) => e.enabled)));
    const el = ref.current;
    const w = el?.offsetWidth ?? 260;
    const h = el?.offsetHeight ?? 300;
    setPos({
      x: Math.max(4, Math.min(lastPointer.x + 4, surface.innerWidth - w - 4)),
      y: Math.max(4, Math.min(lastPointer.y + 4, surface.innerHeight - h - 4)),
    });
  }, [prefix, entries]);

  useEffect(() => {
    if (!prefix) return;
    const onKey = (e: KeyboardEvent) => {
      const step = (d: 1 | -1) => {
        if (!entries.some((x) => x.enabled)) return;
        let i = index;
        do i = (i + d + entries.length) % entries.length;
        while (!entries[i].enabled);
        setIndex(i);
      };
      if (e.key === "ArrowDown") step(1);
      else if (e.key === "ArrowUp") step(-1);
      else if (e.key === "Home") setIndex(Math.max(0, entries.findIndex((x) => x.enabled)));
      else if (e.key === "End") setIndex(entries.length - 1 - [...entries].reverse().findIndex((x) => x.enabled));
      else if (e.key === "Enter") {
        const entry = entries[index];
        if (entry?.enabled) runFromMenu(entry.id);
      } else return;
      e.preventDefault();
      e.stopPropagation();
    };
    const onDown = (e: PointerEvent) => {
      if (ref.current?.contains(e.target as Node)) return;
      // A click outside only closes the menu; it must not also place or select.
      e.stopPropagation();
      e.preventDefault();
      setPending(null);
    };
    surface.addEventListener("keydown", onKey, true);
    surface.addEventListener("pointerdown", onDown, true);
    return () => {
      surface.removeEventListener("keydown", onKey, true);
      surface.removeEventListener("pointerdown", onDown, true);
    };
  }, [prefix, entries, index]);

  if (!prefix) return null;
  const title = TITLES[prefix] ?? prefix;
  return createPortal(
    <div
      ref={ref}
      role="menu"
      className="fixed z-50 min-w-60 overflow-hidden rounded-[5px] border border-line bg-popover py-1 text-xs text-popover-foreground shadow-lg"
      style={{ left: pos.x, top: pos.y }}
    >
      <div className="flex items-center gap-2 border-b border-line px-2.5 pb-1.5 pt-0.5">
        <kbd className="grid size-[18px] place-items-center rounded-[3px] bg-primary font-mono text-[11px] font-semibold text-primary-foreground">{prefix}</kbd>
        <span className="eyebrow">{title}</span>
        <span className="ml-auto text-[10px] text-faint">{activeDoc()?.info.title ?? ""}</span>
      </div>
      {entries.length === 0 && <div className="px-2.5 py-2 text-muted-foreground">Nothing under {prefix} here.</div>}
      {entries.map((e, i) => (
        <button
          key={e.key}
          role="menuitem"
          disabled={!e.enabled}
          onMouseEnter={() => e.enabled && setIndex(i)}
          onClick={() => runFromMenu(e.id)}
          className={cn(
            "flex w-full items-center gap-2.5 px-2.5 py-[3px] text-left",
            e.enabled ? "cursor-default" : "opacity-40",
            i === index && e.enabled && "bg-sel text-accent-ink",
          )}
        >
          <kbd className="grid size-[17px] shrink-0 place-items-center rounded-[3px] border border-line bg-secondary font-mono text-[10px] font-semibold">{e.key}</kbd>
          <span className="truncate">{e.label}</span>
        </button>
      ))}
      <div className="mt-1 border-t border-line px-2.5 pt-1 text-[10px] text-faint">Press a key · ↑↓ Enter · Esc closes</div>
    </div>, surface.document.body
  );
}
