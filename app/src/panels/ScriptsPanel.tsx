import { useRef, useState } from "react";
import { Play, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { NativeSelect, NativeSelectOption } from "@/components/ui/native-select";
import { Textarea } from "@/components/ui/textarea";
import { cn } from "@/lib/utils";
import { api } from "../core/api";
import { execute } from "../core/actions";
import { bus } from "../core/bus";
import { scheduleAnalysis } from "../core/pcbActions";
import { activeDoc, useEditor } from "../core/store";
import type { Command, Item, ItemId } from "../core/types";
import { NM_PER_MM } from "../core/units";

const st = () => useEditor.getState();

/** The object handed to scripts as `ar`. */
function scriptApi(print: (...v: unknown[]) => void) {
  const doc = () => {
    const d = activeDoc();
    if (!d) throw new Error("no active document");
    return d;
  };
  return {
    mm: (v: number) => Math.round(v * NM_PER_MM),
    toMm: (nm: number) => nm / NM_PER_MM,
    print,
    /** Active document info. */
    get doc() {
      return activeDoc()?.info ?? null;
    },
    /** All items of the active document as [id, item] pairs. */
    items(kind?: Item["kind"]): [ItemId, Item][] {
      return [...doc().items].filter(([, it]) => !kind || it.kind === kind);
    },
    get selection(): ItemId[] {
      return [...(activeDoc()?.selection ?? [])];
    },
    select(ids: ItemId[]) {
      st().patchDoc(doc().info.id, { selection: new Set(ids) });
    },
    /** Runs an undoable command (add / delete / move / rotate / mirror / replace / change). */
    run: scriptApiRun,
    add: (items: Item[]) => scriptApiRun({ op: "add", items }),
    remove: (ids: ItemId[]) => scriptApiRun({ op: "delete", ids }),
    move: (ids: ItemId[], dx: number, dy: number) => scriptApiRun({ op: "move", ids, dx, dy }),
    fit: () => bus.emit("view", { action: "fit" }),
    log: (text: string) => st().log("info", text, { source: "script" }),
    /** Raw backend bindings (pcbDrc, pcbNets, outFab, …). */
    api,
  };
}

async function scriptApiRun(cmd: Command) {
  const d = activeDoc();
  if (!d) throw new Error("no active document");
  const ok = await execute(d.info.id, cmd);
  if (ok && d.info.kind === "pcb") scheduleAnalysis(d.info.id);
  return ok;
}

const EXAMPLES: Record<string, string> = {
  "Count items": `const counts = {};
for (const [, it] of ar.items()) counts[it.kind] = (counts[it.kind] ?? 0) + 1;
print(counts);`,
  "List footprints": `for (const [, f] of ar.items("footprint"))
  print(f.designator.padEnd(8), f.value.padEnd(14), f.footprint, ar.toMm(f.pos.x).toFixed(2), ar.toMm(f.pos.y).toFixed(2));`,
  "Select GND tracks": `const ids = ar.items("track").filter(([, t]) => t.net === "GND").map(([id]) => id);
ar.select(ids);
print(ids.length, "tracks selected");`,
  "Place a 5×5 via grid": `const vias = [];
for (let i = 0; i < 5; i++) for (let j = 0; j < 5; j++)
  vias.push({ kind: "via", pos: { x: ar.mm(i * 1.27), y: ar.mm(j * 1.27) }, diameter: ar.mm(0.6), drill: ar.mm(0.3), net: "GND", fromLayer: 0, toLayer: 1 });
await ar.add(vias);`,
  "Run DRC": `const r = await ar.api.pcbDrc(ar.doc.id);
print(r.violations.length, "violations");
for (const v of r.violations.slice(0, 20)) print("-", v.rule, v.message);`,
};

interface Line {
  kind: "in" | "out" | "err";
  text: string;
}

const fmt = (v: unknown) => (typeof v === "string" ? v : (() => {
  try {
    return JSON.stringify(v, null, 1);
  } catch {
    return String(v);
  }
})());

export function ScriptsPanel() {
  const [code, setCode] = useState(EXAMPLES["Count items"]);
  const [lines, setLines] = useState<Line[]>([]);
  const [busy, setBusy] = useState(false);
  const out = useRef<HTMLDivElement>(null);

  const push = (l: Line) => {
    setLines((x) => [...x.slice(-500), l]);
    requestAnimationFrame(() => out.current?.scrollTo({ top: out.current.scrollHeight }));
  };

  const run = async () => {
    setBusy(true);
    push({ kind: "in", text: code.split("\n")[0] + (code.includes("\n") ? " …" : "") });
    const print = (...v: unknown[]) => push({ kind: "out", text: v.map(fmt).join(" ") });
    try {
      const AsyncFunction = Object.getPrototypeOf(async () => {}).constructor as new (...args: string[]) => (...a: unknown[]) => Promise<unknown>;
      const fn = new AsyncFunction("ar", "print", code);
      const result = await fn(scriptApi(print), print);
      if (result !== undefined) print(result);
    } catch (e) {
      push({ kind: "err", text: e instanceof Error ? e.message : String(e) });
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex h-full flex-col bg-panel">
      <div className="flex items-center gap-1 border-b border-line bg-panel-head px-2 py-1">
        <Button size="sm" className="h-6 gap-1 px-2 text-[11px]" disabled={busy} onClick={() => void run()} title="Run (Ctrl+Enter)">
          <Play /> Run
        </Button>
        <NativeSelect value="" onChange={(e) => e.target.value && setCode(EXAMPLES[e.target.value])} wrapperClassName="w-40">
          <NativeSelectOption value="">Examples…</NativeSelectOption>
          {Object.keys(EXAMPLES).map((k) => (
            <NativeSelectOption key={k} value={k}>
              {k}
            </NativeSelectOption>
          ))}
        </NativeSelect>
        <Button variant="ghost" size="icon" className="ml-auto size-6" title="Clear output" onClick={() => setLines([])}>
          <Trash2 />
        </Button>
      </div>
      <Textarea
        value={code}
        spellCheck={false}
        onChange={(e) => setCode(e.target.value)}
        onKeyDown={(e) => {
          e.stopPropagation();
          if (e.key === "Enter" && e.ctrlKey) {
            e.preventDefault();
            void run();
          } else if (e.key === "Tab") {
            e.preventDefault();
            const t = e.currentTarget;
            const s = t.selectionStart;
            setCode(code.slice(0, s) + "  " + code.slice(t.selectionEnd));
            requestAnimationFrame(() => t.setSelectionRange(s + 2, s + 2));
          }
        }}
        className="h-[45%] min-h-24 resize-none rounded-none border-0 border-b border-line bg-field font-mono text-xs shadow-none focus-visible:ring-0"
      />
      <div ref={out} className="min-h-0 flex-1 overflow-auto px-2 py-1 font-mono text-[11px] leading-relaxed">
        {!lines.length && <p className="text-faint">`ar` is the scripting API: ar.items(kind), ar.selection, ar.select(ids), ar.add(items), ar.move(ids, dx, dy), ar.run(command), ar.mm(v), ar.api.*. Ctrl+Enter runs.</p>}
        {lines.map((l, i) => (
          <pre key={i} className={cn("break-all whitespace-pre-wrap", l.kind === "in" && "text-faint", l.kind === "err" && "text-err-line")}>
            {l.kind === "in" ? "› " : ""}
            {l.text}
          </pre>
        ))}
      </div>
    </div>
  );
}
