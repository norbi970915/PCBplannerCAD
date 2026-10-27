import { useEffect, useMemo, useRef, useState } from "react";
import { NativeSelect, NativeSelectOption } from "@/components/ui/native-select";
import { Input } from "@/components/ui/input";
import { cn } from "@/lib/utils";
import { api } from "../core/api";
import { navigateTo, report } from "../core/actions";
import { bus } from "../core/bus";
import { findMatches, type FindKind, type FindMatch } from "../core/find";
import { useEditor } from "../core/store";
import { Dialog, useModal } from "./modal";

const KIND_LABEL: Record<FindKind, string> = { footprint: "Footprint", part: "Part", net: "Net", label: "Label", text: "Text" };

async function go(docId: string, m: FindMatch) {
  const st = useEditor.getState();
  try {
    if (m.kind === "net") {
      const ids = st.docs[docId]?.info.kind === "pcb" ? await api.pcbNetItems(docId, m.label) : m.ids;
      st.patchDoc(docId, { selection: new Set(ids), marked: new Set(ids) });
      bus.emit("view", { action: "fitSelection" });
      st.setHint(`Net ${m.label}: ${ids.length} objects · Esc clears`);
    } else {
      await navigateTo({ docId, items: m.ids, pos: m.pos ?? undefined });
      st.setHint(`${KIND_LABEL[m.kind]} ${m.label}${m.detail ? ` · ${m.detail}` : ""}`);
    }
  } catch (e) {
    report(e);
  }
}

export function FindDialog({ docId }: { docId: string }) {
  const doc = useEditor((s) => s.docs[docId]);
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const [kind,setKind] = useState<FindKind | "all">("all");
  const listRef=useRef<HTMLDivElement>(null);
  useEffect(()=>{listRef.current?.querySelector('[aria-selected="true"]')?.scrollIntoView({block:"nearest"});},[active,query,kind]);
  const matches = useMemo(() => (doc ? findMatches(doc.items, query, 200, kind) : []), [doc?.items, query, kind]);
  const close = () => useModal.getState().close();
  const choose = (m: FindMatch | undefined) => {
    if (!m) return;
    close();
    void go(docId, m);
  };
  return (
    <Dialog title="Find" description={doc?.info.kind === "pcb" ? "Footprints by designator, value or footprint; nets; texts. * and ? are wildcards." : "Parts by designator or value; nets, labels and power ports; texts. * and ? are wildcards."}>
      <div className="grid gap-2">
        <NativeSelect aria-label="Find object type" value={kind} onChange={e=>{setKind(e.target.value as FindKind | "all");setActive(0);}}><NativeSelectOption value="all">All object types</NativeSelectOption>{(doc?.info.kind === "pcb" ? ["footprint","net","text"] : ["part","label","text"]).map(k=><NativeSelectOption key={k} value={k}>{KIND_LABEL[k as FindKind]}</NativeSelectOption>)}</NativeSelect>
        <Input
          autoFocus
          placeholder="R12, 10k, GND, U*…"
          value={query}
          onChange={(e) => {
            setQuery(e.target.value);
            setActive(0);
          }}
          onKeyDown={(e) => {
            if (e.key === "ArrowDown" || e.key === "ArrowUp") {
              e.preventDefault();
              setActive((a) => Math.max(0, Math.min(matches.length - 1, a + (e.key === "ArrowDown" ? 1 : -1))));
            } else if (e.key === "Enter") {
              e.preventDefault();
              choose(matches[active]);
            }
          }}
        />
        <div ref={listRef} role="listbox" aria-label="Search results" className="max-h-[50vh] min-h-24 overflow-auto rounded-[5px] border border-line">
          {matches.map((m, i) => (
            <div
              role="option" aria-selected={i === active}
              key={`${m.kind}:${m.ids[0]}:${m.label}`}
              onMouseEnter={() => setActive(i)}
              onClick={() => choose(m)}
              className={cn("grid cursor-default grid-cols-[64px_minmax(0,1fr)] items-baseline gap-2 px-2 py-1", i === active && "bg-sel")}
            >
              <span className="text-[10px] uppercase text-faint">{KIND_LABEL[m.kind]}</span>
              <span className="min-w-0 truncate">
                <span className="font-mono">{m.label}</span>
                {m.detail && <span className="ml-2 text-muted-foreground">{m.detail}</span>}
              </span>
            </div>
          ))}
          {query.trim() && !matches.length && <p className="px-2 py-2 text-muted-foreground">Nothing matches “{query.trim()}”.</p>}
          {!query.trim() && <p className="px-2 py-2 text-muted-foreground">Type to search · ↑↓ to pick · Enter to go</p>}
        </div>
        {matches.length === 200 && <p className="text-[11px] text-faint">Showing the first 200 matches.</p>}
      </div>
    </Dialog>
  );
}

export function openFind() {
  const id = useEditor.getState().activeDocId;
  if (id) useModal.getState().show(<FindDialog docId={id} />);
}
