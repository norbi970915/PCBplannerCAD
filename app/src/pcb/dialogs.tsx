import { useEffect, useMemo, useState } from "react";
import { open as openDialogPicker, save as saveDialog } from "@tauri-apps/plugin-dialog";
import { ArrowRight, Plus, Trash2 } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { NativeSelect, NativeSelectOption } from "@/components/ui/native-select";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Textarea } from "@/components/ui/textarea";
import { LengthInput } from "@/components/app/fields";
import { cn } from "@/lib/utils";
import { api, type FabOptions } from "../core/api";
import { execute, report } from "../core/actions";
import { bus } from "../core/bus";
import { refreshFootprints, scheduleAnalysis } from "../core/pcbActions";
import { activeDoc, useEditor, type DocState } from "../core/store";
import type { BoardItem, EcoChange, ItemId, NetClass, Rule, StackLayer } from "../core/types";
import { NM_PER_MM } from "../core/units";
import { Dialog, useModal } from "../shell/modal";
import { buildPrint } from "./print";

const mm = (v: number) => Math.round(v * NM_PER_MM);
const toMm = (nm: number) => nm / NM_PER_MM;
const close = () => useModal.getState().close();

type DialogName = "eco" | "stack" | "rules" | "impedance" | "tune" | "outputs" | "print" | "panelize" | "variants";

export function openDialog(name: DialogName) {
  const doc = activeDoc();
  const needsBoard = name !== "impedance" && name !== "print" && name !== "variants";
  if (needsBoard && doc?.info.kind !== "pcb") {
    useEditor.getState().log("warning", "Open a board (PCB document) first.");
    return;
  }
  const id = doc?.info.id ?? "";
  const show = useModal.getState().show;
  switch (name) {
    case "eco":
      return show(<EcoDialog docId={id} />);
    case "stack":
      return show(<StackDialog docId={id} />);
    case "rules":
      return show(<RulesDialog docId={id} />);
    case "impedance":
      return show(<ImpedanceDialog docId={doc?.info.kind === "pcb" ? id : null} />);
    case "tune":
      return show(<TuneDialog docId={id} />);
    case "outputs":
      return show(<OutputsDialog docId={id} />);
    case "print":
      return doc ? show(<PrintDialog docId={id} />) : undefined;
    case "panelize":
      return show(<PanelizeDialog docId={id} />);
    case "variants":
      return show(<VariantsDialog />);
  }
}

/** The board-settings item of a PCB document. */
function useBoard(docId: string): [ItemId, BoardItem] | null {
  const doc = useEditor((s) => s.docs[docId]);
  return useMemo(() => {
    if (!doc) return null;
    for (const [id, it] of doc.items) if (it.kind === "board") return [id, it] as [ItemId, BoardItem];
    return null;
  }, [doc]);
}

async function saveBoard(docId: string, id: ItemId, board: BoardItem) {
  if (await execute(docId, { op: "replace", id, item: board })) scheduleAnalysis(docId, 0);
}

function Field({ label, children, className }: { label: string; children: React.ReactNode; className?: string }) {
  return (
    <label className={cn("grid gap-1", className)}>
      <span className="text-[11px] text-muted-foreground">{label}</span>
      {children}
    </label>
  );
}

// ---------- Update PCB from schematic ----------

function describe(c: EcoChange): { action: string; target: string; detail: string } {
  switch (c.kind) {
    case "addFootprint":
      return { action: "Add", target: c.designator, detail: `${c.value} · ${c.footprintKey}` };
    case "removeFootprint":
      return { action: "Remove", target: c.designator, detail: "not in schematic" };
    case "changeFootprint":
      return { action: "Change footprint", target: c.designator, detail: `${c.from} → ${c.to}` };
    case "updateValue":
      return { action: "Update value", target: c.designator, detail: `${c.from} → ${c.to}` };
    case "updateNets":
      return { action: "Update nets", target: c.designator, detail: c.changes.map(([p, a, b]) => `${p}: ${a || "—"} → ${b || "—"}`).join(", ") };
    case "updateParameters":
      return { action: "Update parameters", target: c.designator, detail: "" };
    case "renameNet":
      return { action: "Rename net", target: c.from, detail: `→ ${c.to}` };
    default:
      return { action: "Warning", target: "", detail: c.message };
  }
}

function EcoDialog({ docId }: { docId: string }) {
  const [changes, setChanges] = useState<EcoChange[] | null>(null);
  const [selected, setSelected] = useState<Set<number>>(new Set());
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api
      .pcbEcoPlan(docId)
      .then((c) => {
        setChanges(c);
        setSelected(new Set(c.map((x, i) => (x.kind === "warning" ? -1 : i)).filter((i) => i >= 0)));
      })
      .catch((e) => setError(String(e)));
  }, [docId]);

  const apply = async () => {
    try {
      const u = await api.pcbEcoApply(docId, [...selected]);
      useEditor.getState().applyUpdate(u);
      await refreshFootprints(docId);
      scheduleAnalysis(docId, 0);
      useEditor.getState().log("info", `Applied ${selected.size} engineering changes.`);
      bus.emit("view", { action: "fit" });
      close();
    } catch (e) {
      report(e);
    }
  };

  const actionable = changes?.filter((c) => c.kind !== "warning").length ?? 0;
  return (
    <Dialog
      title="Engineering change order"
      description="Changes needed to make the board match the project schematics."
      wide
      footer={
        <>
          <span className="mr-auto text-[11px] text-muted-foreground">{changes ? `${selected.size} of ${actionable} changes selected` : ""}</span>
          <Button variant="outline" onClick={close}>
            Cancel
          </Button>
          <Button disabled={!selected.size} onClick={() => void apply()}>
            Execute changes
          </Button>
        </>
      }
    >
      {error && <p className="text-err-line">{error}</p>}
      {!changes && !error && <p className="text-muted-foreground">Comparing schematics and board…</p>}
      {changes && !changes.length && <p className="text-muted-foreground">The board is already in sync with the schematics.</p>}
      {changes && changes.length > 0 && (
        <div className="max-h-[55vh] overflow-auto rounded-[5px] border border-line">
          <div className="sticky top-0 grid grid-cols-[28px_150px_110px_1fr] gap-2 border-b border-line bg-panel-head px-2 py-1.5 text-[11px] font-semibold text-faint">
            <Checkbox
              className="size-3.5"
              checked={selected.size === actionable && actionable > 0}
              onCheckedChange={(v) => setSelected(v === true ? new Set(changes.map((c, i) => (c.kind === "warning" ? -1 : i)).filter((i) => i >= 0)) : new Set())}
            />
            <span>Action</span>
            <span>Object</span>
            <span>Details</span>
          </div>
          {changes.map((c, i) => {
            const d = describe(c);
            const warn = c.kind === "warning";
            return (
              <div key={i} className={cn("grid grid-cols-[28px_150px_110px_1fr] items-start gap-2 border-b border-line/60 px-2 py-1 text-xs", warn && "bg-note/60")}>
                {warn ? (
                  <span />
                ) : (
                  <Checkbox
                    className="mt-0.5 size-3.5"
                    checked={selected.has(i)}
                    onCheckedChange={(v) => {
                      const next = new Set(selected);
                      if (v === true) next.add(i);
                      else next.delete(i);
                      setSelected(next);
                    }}
                  />
                )}
                <span className={cn(warn ? "text-note-line" : c.kind === "removeFootprint" ? "text-err-line" : c.kind === "addFootprint" ? "text-ok" : "")}>{d.action}</span>
                <span className="font-mono">{d.target}</span>
                <span className="text-muted-foreground">{d.detail}</span>
              </div>
            );
          })}
        </div>
      )}
    </Dialog>
  );
}

// ---------- Layer stack ----------

function StackDialog({ docId }: { docId: string }) {
  const board = useBoard(docId);
  const [layers, setLayers] = useState<StackLayer[]>(board?.[1].stackup.layers ?? []);
  const [mask, setMask] = useState(board?.[1].maskColor ?? "#1f5f2a");
  const [silk, setSilk] = useState(board?.[1].silkColor ?? "#f2f2f2");
  if (!board) return null;
  const [id, settings] = board;
  const copper = layers.filter((l) => l.kind === "copper").length;
  const total = layers.reduce((s, l) => s + l.thickness, 0);

  /** Regenerates a standard stack with `n` copper layers (keeps names of existing ones). */
  const setCopper = (n: number) => {
    const cu = toMm(0.035 * NM_PER_MM);
    const diel = (1.6 - cu * n - 0.02) / Math.max(1, n - 1);
    const next: StackLayer[] = [
      { name: "Top Overlay", kind: "silkscreen", layer: 2, thickness: 0, material: "", epsilonR: 0 },
      { name: "Top Paste", kind: "paste", layer: 8, thickness: 0, material: "", epsilonR: 0 },
      { name: "Top Solder", kind: "solderMask", layer: 6, thickness: mm(0.01), material: "Solder Resist", epsilonR: 3.5 },
    ];
    for (let i = 0; i < n; i++) {
      const layer = i === 0 ? 0 : i === n - 1 ? 1 : 32 + i;
      next.push({ name: i === 0 ? "Top Layer" : i === n - 1 ? "Bottom Layer" : `Inner ${i}`, kind: "copper", layer, thickness: mm(0.035), material: "Copper", epsilonR: 0 });
      if (i + 1 < n) next.push({ name: `Dielectric ${i + 1}`, kind: "dielectric", layer: null, thickness: mm(diel), material: i % 2 === 0 ? "FR-4 Core" : "FR-4 Prepreg", epsilonR: 4.2 });
    }
    next.push(
      { name: "Bottom Solder", kind: "solderMask", layer: 7, thickness: mm(0.01), material: "Solder Resist", epsilonR: 3.5 },
      { name: "Bottom Paste", kind: "paste", layer: 9, thickness: 0, material: "", epsilonR: 0 },
      { name: "Bottom Overlay", kind: "silkscreen", layer: 3, thickness: 0, material: "", epsilonR: 0 },
    );
    setLayers(next);
  };

  const patch = (i: number, p: Partial<StackLayer>) => setLayers(layers.map((l, j) => (j === i ? { ...l, ...p } : l)));
  const save = async () => {
    await saveBoard(docId, id, { ...settings, stackup: { layers }, maskColor: mask, silkColor: silk });
    close();
  };

  return (
    <Dialog
      title="Layer stack manager"
      wide
      footer={
        <>
          <span className="mr-auto font-mono text-[11px] text-muted-foreground">
            {copper} copper layers · {toMm(total).toFixed(3)} mm total
          </span>
          <Button variant="outline" onClick={close}>
            Cancel
          </Button>
          <Button onClick={() => void save()}>Apply</Button>
        </>
      }
    >
      <div className="mb-3 flex flex-wrap items-end gap-3">
        <Field label="Copper layers">
          <NativeSelect value={String(copper)} onChange={(e) => setCopper(Number(e.target.value))} wrapperClassName="w-28">
            {[1, 2, 4, 6, 8, 10, 12, 14, 16].map((n) => (
              <NativeSelectOption key={n} value={String(n)}>
                {n}
              </NativeSelectOption>
            ))}
          </NativeSelect>
        </Field>
        <Field label="Solder mask color">
          <input type="color" value={mask} onChange={(e) => setMask(e.target.value)} className="h-7 w-16 rounded-[5px] border border-line bg-field" />
        </Field>
        <Field label="Silkscreen color">
          <input type="color" value={silk} onChange={(e) => setSilk(e.target.value)} className="h-7 w-16 rounded-[5px] border border-line bg-field" />
        </Field>
      </div>
      <div className="rounded-[5px] border border-line">
        <div className="grid grid-cols-[18px_1.3fr_110px_110px_1fr_70px] gap-2 border-b border-line bg-panel-head px-2 py-1.5 text-[11px] font-semibold text-faint">
          <span />
          <span>Name</span>
          <span>Type</span>
          <span>Thickness (mm)</span>
          <span>Material</span>
          <span>εr</span>
        </div>
        {layers.map((l, i) => (
          <div key={i} className="grid grid-cols-[18px_1.3fr_110px_110px_1fr_70px] items-center gap-2 border-b border-line/60 px-2 py-0.5">
            <span
              className="h-3 w-3 rounded-[2px]"
              style={{ background: l.kind === "copper" ? "var(--ui-copper)" : l.kind === "dielectric" ? "var(--ui-laminate)" : l.kind === "solderMask" ? mask : l.kind === "silkscreen" ? silk : "#9aa0a8" }}
            />
            <Input value={l.name} onChange={(e) => patch(i, { name: e.target.value })} className="h-6" />
            <span className="text-muted-foreground capitalize">{l.kind.replace("solderMask", "solder mask")}</span>
            <Input className="h-6 font-mono" value={toMm(l.thickness).toString()} onChange={(e) => Number.isFinite(parseFloat(e.target.value)) && patch(i, { thickness: mm(parseFloat(e.target.value)) })} />
            <Input value={l.material} onChange={(e) => patch(i, { material: e.target.value })} className="h-6" />
            <Input className="h-6 font-mono" value={l.epsilonR ? String(l.epsilonR) : ""} disabled={l.kind !== "dielectric" && l.kind !== "solderMask"} onChange={(e) => patch(i, { epsilonR: parseFloat(e.target.value) || 0 })} />
          </div>
        ))}
      </div>
    </Dialog>
  );
}

// ---------- Rules & net classes ----------

const RULE_LABEL: Record<Rule["type"], string> = {
  clearance: "Clearance",
  width: "Track width",
  viaStyle: "Via style",
  holeSize: "Hole size",
  boardOutlineClearance: "Board outline clearance",
  componentClearance: "Component clearance",
  solderMaskExpansion: "Solder mask expansion",
  pasteMaskExpansion: "Paste mask expansion",
  zoneConnect: "Polygon connect style",
  diffPair: "Differential pair",
  length: "Length",
  silkClearance: "Silk to pad clearance",
};

const LENGTH_KEYS = ["min", "preferred", "max", "diameter", "drill", "minAnnularRing", "expansion", "gap", "spokeWidth", "width"] as const;
const SCOPE_KEYS = ["first", "second", "scope"] as const;

function newRule(type: Rule["type"]): Rule {
  const base = { name: RULE_LABEL[type], enabled: true };
  switch (type) {
    case "clearance":
      return { ...base, type, first: "All", second: "All", min: mm(0.2) };
    case "width":
      return { ...base, type, scope: "All", min: mm(0.15), preferred: mm(0.25), max: mm(3) };
    case "viaStyle":
      return { ...base, type, scope: "All", diameter: mm(0.6), drill: mm(0.3), minAnnularRing: mm(0.13) };
    case "holeSize":
      return { ...base, type, scope: "All", min: mm(0.2), max: mm(6.3) };
    case "length":
      return { ...base, type, scope: "InNetClass('Default')", min: 0, max: mm(100) };
    case "diffPair":
      return { ...base, type, scope: "All", width: mm(0.2), gap: mm(0.2) };
    case "zoneConnect":
      return { ...base, type, scope: "All", style: "thermal", gap: mm(0.3), spokeWidth: mm(0.3) };
    case "solderMaskExpansion":
    case "pasteMaskExpansion":
      return { ...base, type, scope: "All", expansion: 0 };
    default:
      return { ...base, type, min: mm(0.2) } as Rule;
  }
}

function RulesDialog({ docId }: { docId: string }) {
  const board = useBoard(docId);
  const units = useEditor((s) => s.units);
  const [rules, setRules] = useState<Rule[]>(board?.[1].rules ?? []);
  const [classes, setClasses] = useState<NetClass[]>(board?.[1].netClasses ?? []);
  const [pairs, setPairs] = useState(board?.[1].diffPairs ?? []);
  const [current, setCurrent] = useState(0);
  const [addType, setAddType] = useState<Rule["type"]>("clearance");
  if (!board) return null;
  const [id, settings] = board;
  const rule = rules[current];
  const patchRule = (p: Partial<Rule>) => setRules(rules.map((r, i) => (i === current ? ({ ...r, ...p } as Rule) : r)));
  const patchClass = (i: number, p: Partial<NetClass>) => setClasses(classes.map((c, j) => (j === i ? { ...c, ...p } : c)));
  const move = (d: -1 | 1) => {
    const j = current + d;
    if (j < 0 || j >= rules.length) return;
    const next = [...rules];
    [next[current], next[j]] = [next[j], next[current]];
    setRules(next);
    setCurrent(j);
  };
  const save = async () => {
    await saveBoard(docId, id, { ...settings, rules, netClasses: classes, diffPairs: pairs });
    close();
  };

  return (
    <Dialog
      title="Rules & net classes"
      description="Rules are checked top to bottom; the first enabled rule whose scope matches wins. Scopes: All, InNet('GND'), InNetClass('Power'), OnLayer('Top Layer'), IsPad, IsVia, InComponent('U1'), HasFootprint('SOIC*') — combine with and / or / not."
      wide
      footer={
        <>
          <Button variant="outline" onClick={close}>
            Cancel
          </Button>
          <Button onClick={() => void save()}>Apply</Button>
        </>
      }
    >
      <Tabs defaultValue="rules">
        <TabsList>
          <TabsTrigger value="rules">Design rules</TabsTrigger>
          <TabsTrigger value="classes">Net classes</TabsTrigger>
          <TabsTrigger value="pairs">Differential pairs</TabsTrigger>
        </TabsList>
        <TabsContent value="rules" className="mt-3">
          <div className="grid grid-cols-[260px_minmax(0,1fr)] gap-3">
            <div className="grid content-start gap-1.5">
              <div className="max-h-80 overflow-auto rounded-[5px] border border-line bg-field py-0.5">
                {rules.map((r, i) => (
                  <div key={i} onClick={() => setCurrent(i)} className={cn("mx-0.5 flex cursor-default items-center gap-2 rounded-[4px] px-2 py-1 hover:bg-hover", i === current && "bg-sel hover:bg-sel", !r.enabled && "opacity-50")}>
                    <span className="font-mono text-[10px] text-faint">{i + 1}</span>
                    <span className="truncate">{r.name}</span>
                    <span className="ml-auto text-[10px] text-faint">{RULE_LABEL[r.type]}</span>
                  </div>
                ))}
              </div>
              <div className="flex gap-1">
                <NativeSelect value={addType} onChange={(e) => setAddType(e.target.value as Rule["type"])}>
                  {Object.entries(RULE_LABEL).map(([k, v]) => (
                    <NativeSelectOption key={k} value={k}>
                      {v}
                    </NativeSelectOption>
                  ))}
                </NativeSelect>
                <Button variant="outline" size="icon" title="Add rule" onClick={() => (setRules([...rules, newRule(addType)]), setCurrent(rules.length))}>
                  <Plus />
                </Button>
              </div>
              <div className="flex gap-1">
                <Button variant="outline" size="sm" onClick={() => move(-1)}>
                  Up
                </Button>
                <Button variant="outline" size="sm" onClick={() => move(1)}>
                  Down
                </Button>
                <Button variant="outline" size="sm" disabled={!rule} onClick={() => (setRules(rules.filter((_, i) => i !== current)), setCurrent(Math.max(0, current - 1)))}>
                  <Trash2 /> Delete
                </Button>
              </div>
            </div>
            {rule && (
              <div className="grid content-start gap-2">
                <div className="grid grid-cols-[1fr_auto] items-end gap-3">
                  <Field label="Name">
                    <Input value={rule.name} onChange={(e) => patchRule({ name: e.target.value })} />
                  </Field>
                  <label className="flex items-center gap-2 pb-1.5">
                    <Checkbox checked={rule.enabled} onCheckedChange={(v) => patchRule({ enabled: v === true })} /> Enabled
                  </label>
                </div>
                <Badge variant="secondary" className="w-fit">
                  {RULE_LABEL[rule.type]}
                </Badge>
                {SCOPE_KEYS.filter((k) => k in rule).map((k) => (
                  <Field key={k} label={k === "first" ? "First object matches" : k === "second" ? "Second object matches" : "Applies to"}>
                    <Input className="font-mono" value={(rule as Record<string, unknown>)[k] as string} onChange={(e) => patchRule({ [k]: e.target.value } as Partial<Rule>)} />
                  </Field>
                ))}
                <div className="grid grid-cols-3 gap-2">
                  {LENGTH_KEYS.filter((k) => k in rule).map((k) => (
                    <Field key={k} label={k.replace(/([A-Z])/g, " $1").toLowerCase()}>
                      <LengthInput value={(rule as Record<string, unknown>)[k] as number} units={units} onCommit={(v) => patchRule({ [k]: v } as Partial<Rule>)} />
                    </Field>
                  ))}
                  {rule.type === "zoneConnect" && (
                    <Field label="style">
                      <NativeSelect value={rule.style} onChange={(e) => patchRule({ style: e.target.value } as Partial<Rule>)}>
                        <NativeSelectOption value="thermal">Thermal relief</NativeSelectOption>
                        <NativeSelectOption value="solid">Direct (solid)</NativeSelectOption>
                        <NativeSelectOption value="none">No connect</NativeSelectOption>
                      </NativeSelect>
                    </Field>
                  )}
                </div>
              </div>
            )}
          </div>
        </TabsContent>
        <TabsContent value="classes" className="mt-3">
          <div className="grid gap-1">
            <div className="grid grid-cols-[110px_repeat(6,90px)_1fr_24px] gap-1.5 px-1 text-[11px] font-semibold text-faint">
              <span>Class</span>
              <span>Track</span>
              <span>Clearance</span>
              <span>Via Ø</span>
              <span>Via drill</span>
              <span>DP width</span>
              <span>DP gap</span>
              <span>Nets (comma separated)</span>
              <span />
            </div>
            {classes.map((c, i) => (
              <div key={i} className="grid grid-cols-[110px_repeat(6,90px)_1fr_24px] items-center gap-1.5">
                <Input value={c.name} onChange={(e) => patchClass(i, { name: e.target.value })} />
                <LengthInput value={c.trackWidth} units={units} onCommit={(v) => patchClass(i, { trackWidth: v })} />
                <LengthInput value={c.clearance} units={units} onCommit={(v) => patchClass(i, { clearance: v })} />
                <LengthInput value={c.viaDiameter} units={units} onCommit={(v) => patchClass(i, { viaDiameter: v })} />
                <LengthInput value={c.viaDrill} units={units} onCommit={(v) => patchClass(i, { viaDrill: v })} />
                <LengthInput value={c.diffPairWidth} units={units} onCommit={(v) => patchClass(i, { diffPairWidth: v })} />
                <LengthInput value={c.diffPairGap} units={units} onCommit={(v) => patchClass(i, { diffPairGap: v })} />
                <Input className="font-mono" value={c.nets.join(", ")} placeholder={i === 0 ? "every other net" : "VCC, GND"} onChange={(e) => patchClass(i, { nets: e.target.value.split(",").map((s) => s.trim()).filter(Boolean) })} />
                <Button variant="ghost" size="icon" disabled={i === 0} onClick={() => setClasses(classes.filter((_, j) => j !== i))}>
                  <Trash2 />
                </Button>
              </div>
            ))}
            <Button variant="outline" size="sm" className="mt-1 w-fit" onClick={() => setClasses([...classes, { ...classes[0], name: `Class${classes.length}`, nets: [] }])}>
              <Plus /> Net class
            </Button>
            <p className="pt-1 text-[11px] text-faint">The first class is the default for nets not listed anywhere.</p>
          </div>
        </TabsContent>
        <TabsContent value="pairs" className="mt-3">
          <div className="grid gap-1">
            {pairs.map((p, i) => (
              <div key={i} className="grid grid-cols-[1fr_1fr_1fr_24px] gap-1.5">
                <Input value={p.name} placeholder="Name" onChange={(e) => setPairs(pairs.map((x, j) => (j === i ? { ...x, name: e.target.value } : x)))} />
                <Input className="font-mono" value={p.positive} placeholder="Positive net" onChange={(e) => setPairs(pairs.map((x, j) => (j === i ? { ...x, positive: e.target.value } : x)))} />
                <Input className="font-mono" value={p.negative} placeholder="Negative net" onChange={(e) => setPairs(pairs.map((x, j) => (j === i ? { ...x, negative: e.target.value } : x)))} />
                <Button variant="ghost" size="icon" onClick={() => setPairs(pairs.filter((_, j) => j !== i))}>
                  <Trash2 />
                </Button>
              </div>
            ))}
            <Button variant="outline" size="sm" className="w-fit" onClick={() => setPairs([...pairs, { name: `DP${pairs.length + 1}`, positive: "", negative: "" }])}>
              <Plus /> Differential pair
            </Button>
            <p className="pt-1 text-[11px] text-faint">Nets ending in _P/_N or +/- pair up automatically when routing with P I.</p>
          </div>
        </TabsContent>
      </Tabs>
    </Dialog>
  );
}

// ---------- Impedance calculator (IPC-2141A approximations) ----------

function microstrip(w: number, h: number, t: number, er: number) {
  return (87 / Math.sqrt(er + 1.41)) * Math.log((5.98 * h) / (0.8 * w + t));
}

function stripline(w: number, b: number, t: number, er: number) {
  return (60 / Math.sqrt(er)) * Math.log((1.9 * b) / (0.8 * w + t));
}

function solveWidth(f: (w: number) => number, target: number): number | null {
  let lo = 0.01;
  let hi = 20;
  if (f(lo) < target || f(hi) > target) return null;
  for (let i = 0; i < 60; i++) {
    const mid = (lo + hi) / 2;
    if (f(mid) > target) lo = mid;
    else hi = mid;
  }
  return (lo + hi) / 2;
}

function ImpedanceDialog({ docId }: { docId: string | null }) {
  const board = useBoard(docId ?? "");
  const firstDielectric = board?.[1].stackup.layers.find((l) => l.kind === "dielectric");
  const [kind, setKind] = useState<"microstrip" | "stripline">("microstrip");
  const [w, setW] = useState("0.3");
  const [h, setH] = useState(firstDielectric ? String(toMm(firstDielectric.thickness).toFixed(3)) : "0.2");
  const [t, setT] = useState("0.035");
  const [er, setEr] = useState(firstDielectric ? String(firstDielectric.epsilonR || 4.2) : "4.2");
  const [target, setTarget] = useState("50");
  const n = (s: string) => parseFloat(s) || 0;
  const f = (width: number) => (kind === "microstrip" ? microstrip(width, n(h), n(t), n(er)) : stripline(width, n(h), n(t), n(er)));
  const z = f(n(w));
  const solved = solveWidth(f, n(target));

  return (
    <Dialog title="Impedance calculator" description="IPC-2141A closed-form approximations (±10% typical) — confirm critical lines with your fab's field solver." footer={<Button onClick={close}>Close</Button>}>
      <div className="grid gap-3">
        <Tabs value={kind} onValueChange={(v) => setKind(v as typeof kind)}>
          <TabsList>
            <TabsTrigger value="microstrip">Microstrip (outer layer)</TabsTrigger>
            <TabsTrigger value="stripline">Stripline (inner layer)</TabsTrigger>
          </TabsList>
        </Tabs>
        <div className="grid grid-cols-2 gap-3">
          <Field label="Track width w (mm)">
            <Input className="font-mono" value={w} onChange={(e) => setW(e.target.value)} />
          </Field>
          <Field label={kind === "microstrip" ? "Dielectric height h (mm)" : "Plane spacing b (mm)"}>
            <Input className="font-mono" value={h} onChange={(e) => setH(e.target.value)} />
          </Field>
          <Field label="Copper thickness t (mm)">
            <Input className="font-mono" value={t} onChange={(e) => setT(e.target.value)} />
          </Field>
          <Field label="Relative permittivity εr">
            <Input className="font-mono" value={er} onChange={(e) => setEr(e.target.value)} />
          </Field>
        </div>
        <div className="rounded-[5px] border border-line bg-panel-head p-3">
          <div className="eyebrow">Characteristic impedance</div>
          <div className="font-mono text-[22px] font-semibold">{Number.isFinite(z) && z > 0 ? `${z.toFixed(1)} Ω` : "—"}</div>
        </div>
        <div className="flex items-end gap-2">
          <Field label="Target Z₀ (Ω)" className="w-32">
            <Input className="font-mono" value={target} onChange={(e) => setTarget(e.target.value)} />
          </Field>
          <ArrowRight className="mb-1.5 size-4 text-faint" />
          <span className="pb-1.5 font-mono">{solved ? `w ≈ ${solved.toFixed(3)} mm` : "out of range"}</span>
          {solved && (
            <Button variant="outline" size="sm" className="mb-0.5 ml-auto" onClick={() => setW(solved.toFixed(3))}>
              Use width
            </Button>
          )}
        </div>
      </div>
    </Dialog>
  );
}

// ---------- Length tuning ----------

function TuneDialog({ docId }: { docId: string }) {
  const doc = useEditor((s) => s.docs[docId]);
  const units = useEditor((s) => s.units);
  const tracks = useMemo(() => [...(doc?.selection ?? [])].filter((id) => doc?.items.get(id)?.kind === "track"), [doc]);
  const current = useMemo(() => {
    let sum = 0;
    for (const id of tracks) {
      const t = doc?.items.get(id);
      if (t?.kind === "track") sum += Math.hypot(t.b.x - t.a.x, t.b.y - t.a.y);
    }
    return sum;
  }, [tracks, doc]);
  const [target, setTarget] = useState(Math.round(current * 1.1));
  const [amplitude, setAmplitude] = useState(mm(1));
  const [spacing, setSpacing] = useState(mm(0.5));
  const [style, setStyle] = useState<"accordion" | "trombone">("accordion");

  const run = async () => {
    try {
      useEditor.getState().applyUpdate(await api.pcbTune(docId, { tracks, targetLength: target, amplitude, spacing, style }));
      scheduleAnalysis(docId, 0);
      close();
    } catch (e) {
      report(e);
    }
  };
  return (
    <Dialog
      title="Length tuning"
      description="Adds meanders to the selected track chain until it reaches the target length."
      footer={
        <>
          <Button variant="outline" onClick={close}>
            Cancel
          </Button>
          <Button disabled={!tracks.length} onClick={() => void run()}>
            Tune
          </Button>
        </>
      }
    >
      <div className="grid gap-3">
        <p className="text-muted-foreground">
          {tracks.length} tracks selected · current length <span className="font-mono text-ink">{toMm(current).toFixed(3)} mm</span>
        </p>
        <div className="grid grid-cols-2 gap-3">
          <Field label="Target length">
            <LengthInput value={target} units={units} onCommit={setTarget} />
          </Field>
          <Field label="Style">
            <NativeSelect value={style} onChange={(e) => setStyle(e.target.value as typeof style)}>
              <NativeSelectOption value="accordion">Accordion (45°)</NativeSelectOption>
              <NativeSelectOption value="trombone">Trombone (90°)</NativeSelectOption>
            </NativeSelect>
          </Field>
          <Field label="Amplitude">
            <LengthInput value={amplitude} units={units} onCommit={setAmplitude} />
          </Field>
          <Field label="Spacing">
            <LengthInput value={spacing} units={units} onCommit={setSpacing} />
          </Field>
        </div>
      </div>
    </Dialog>
  );
}

// ---------- Fabrication outputs ----------

function OutputsDialog({ docId }: { docId: string }) {
  const project = useEditor((s) => s.project);
  const [opts, setOpts] = useState<FabOptions>({
    gerber: { tentVias: true, includeRefDes: true, includeValues: false, plotOutlineOnAll: false, precision: 6, projectName: project?.name ?? "" },
    pnp: "jlc",
    bom: "jlc",
    zip: true,
  });
  const [variant, setVariant] = useState<string>("");
  const [files, setFiles] = useState<string[] | null>(null);
  const [busy, setBusy] = useState(false);
  const g = (p: Partial<FabOptions["gerber"]>) => setOpts({ ...opts, gerber: { ...opts.gerber, ...p } });
  const v = variant || null;

  const runFab = async () => {
    const dir = await openDialogPicker({ directory: true, title: "Folder for fabrication files" });
    if (typeof dir !== "string") return;
    setBusy(true);
    try {
      const out = await api.outFab(docId, dir, opts, v);
      setFiles(out);
      useEditor.getState().log("info", `Wrote ${out.length} fabrication files to ${dir}`);
    } catch (e) {
      report(e);
    } finally {
      setBusy(false);
    }
  };
  const single = async (kind: "pnp" | "bom" | "ipc") => {
    const ext = kind === "ipc" ? "xml" : "csv";
    const path = await saveDialog({ filters: [{ name: ext.toUpperCase(), extensions: [ext] }], defaultPath: `${project?.name ?? "board"}_${kind}.${ext}` });
    if (!path) return;
    try {
      if (kind === "pnp") await api.outPnp(docId, path, opts.pnp ?? "generic", v);
      else if (kind === "bom") await api.outBoardBom(docId, path, opts.bom ?? "generic", v);
      else await api.outIpc2581(docId, path);
      useEditor.getState().log("info", `Wrote ${path}`);
    } catch (e) {
      report(e);
    }
  };

  return (
    <Dialog
      title="Fabrication outputs"
      description="Gerber X2, Excellon drills and assembly files for your fab house."
      wide
      footer={
        <>
          {files && (
            <Button variant="outline" className="mr-auto" onClick={() => (bus.emit("showGerbers", { path: files[files.length - 1] }), close())}>
              View in Gerber viewer
            </Button>
          )}
          <Button variant="outline" onClick={close}>
            Close
          </Button>
          <Button disabled={busy} onClick={() => void runFab()}>
            {busy ? "Generating…" : "Generate fab package…"}
          </Button>
        </>
      }
    >
      <div className="grid grid-cols-2 gap-4">
        <div className="grid content-start gap-2">
          <div className="eyebrow">Gerber</div>
          {(
            [
              ["tentVias", "Tent vias (cover with solder mask)"],
              ["includeRefDes", "Designators on silkscreen"],
              ["includeValues", "Values on silkscreen"],
              ["plotOutlineOnAll", "Board outline on every layer"],
            ] as const
          ).map(([k, label]) => (
            <label key={k} className="flex items-center gap-2">
              <Checkbox checked={opts.gerber[k]} onCheckedChange={(c) => g({ [k]: c === true })} /> {label}
            </label>
          ))}
          <label className="flex items-center gap-2">
            <Checkbox checked={opts.zip} onCheckedChange={(c) => setOpts({ ...opts, zip: c === true })} /> Also create a .zip for upload
          </label>
        </div>
        <div className="grid content-start gap-2">
          <div className="eyebrow">Assembly</div>
          <Field label="Pick and place">
            <NativeSelect value={opts.pnp ?? ""} onChange={(e) => setOpts({ ...opts, pnp: (e.target.value || null) as FabOptions["pnp"] })}>
              <NativeSelectOption value="">None</NativeSelectOption>
              <NativeSelectOption value="jlc">JLCPCB (CPL)</NativeSelectOption>
              <NativeSelectOption value="coordinates">structured format format</NativeSelectOption>
              <NativeSelectOption value="generic">Generic CSV</NativeSelectOption>
            </NativeSelect>
          </Field>
          <Field label="Bill of materials">
            <NativeSelect value={opts.bom ?? ""} onChange={(e) => setOpts({ ...opts, bom: (e.target.value || null) as FabOptions["bom"] })}>
              <NativeSelectOption value="">None</NativeSelectOption>
              <NativeSelectOption value="jlc">JLCPCB</NativeSelectOption>
              <NativeSelectOption value="generic">Generic CSV</NativeSelectOption>
            </NativeSelect>
          </Field>
          {project && project.variants.length > 0 && (
            <Field label="Variant">
              <NativeSelect value={variant} onChange={(e) => setVariant(e.target.value)}>
                <NativeSelectOption value="">All parts fitted</NativeSelectOption>
                {project.variants.map((x) => (
                  <NativeSelectOption key={x.name} value={x.name}>
                    {x.name}
                  </NativeSelectOption>
                ))}
              </NativeSelect>
            </Field>
          )}
          <div className="flex flex-wrap gap-1.5 pt-1">
            <Button variant="outline" size="sm" onClick={() => void single("pnp")}>
              PnP only…
            </Button>
            <Button variant="outline" size="sm" onClick={() => void single("bom")}>
              BOM only…
            </Button>
            <Button variant="outline" size="sm" onClick={() => void single("ipc")}>
              IPC-2581…
            </Button>
          </div>
        </div>
      </div>
      {files && (
        <div className="mt-3 max-h-40 overflow-auto rounded-[5px] border border-line bg-field p-2 font-mono text-[11px]">
          {files.map((f) => (
            <div key={f} className="truncate" title={f}>
              {f.split(/[\\/]/).pop()}
            </div>
          ))}
        </div>
      )}
    </Dialog>
  );
}

// ---------- Print to PDF ----------

function PrintDialog({ docId }: { docId: string }) {
  const doc = useEditor((s) => s.docs[docId]) as DocState | undefined;
  const [paper, setPaper] = useState<"A4" | "A3" | "Letter">("A4");
  const [landscape, setLandscape] = useState(true);
  const [mono, setMono] = useState(false);
  const run = async () => {
    if (!doc) return;
    const path = await saveDialog({ filters: [{ name: "PDF", extensions: ["pdf"] }], defaultPath: doc.info.title.replace(/\.\w+$/, ".pdf") });
    if (!path) return;
    try {
      const bytes = buildPrint(doc, { paper, landscape, mono });
      await api.writeFileBytes(path, bytes);
      useEditor.getState().log("info", `Printed ${doc.info.title} to ${path}`);
      close();
    } catch (e) {
      report(e);
    }
  };
  return (
    <Dialog
      title="Print to PDF"
      description={doc?.info.kind === "pcb" ? "Board layers as seen from the top, one page." : "The current sheet as vector graphics."}
      footer={
        <>
          <Button variant="outline" onClick={close}>
            Cancel
          </Button>
          <Button onClick={() => void run()}>Save PDF…</Button>
        </>
      }
    >
      <div className="grid gap-3">
        <Field label="Paper">
          <NativeSelect value={paper} onChange={(e) => setPaper(e.target.value as typeof paper)}>
            <NativeSelectOption value="A4">A4</NativeSelectOption>
            <NativeSelectOption value="A3">A3</NativeSelectOption>
            <NativeSelectOption value="Letter">Letter</NativeSelectOption>
          </NativeSelect>
        </Field>
        <label className="flex items-center gap-2">
          <Checkbox checked={landscape} onCheckedChange={(v) => setLandscape(v === true)} /> Landscape
        </label>
        <label className="flex items-center gap-2">
          <Checkbox checked={mono} onCheckedChange={(v) => setMono(v === true)} /> Black &amp; white
        </label>
      </div>
    </Dialog>
  );
}

// ---------- Panelize ----------

function PanelizeDialog({ docId }: { docId: string }) {
  const units = useEditor((s) => s.units);
  const [rows, setRows] = useState("2");
  const [cols, setCols] = useState("2");
  const [spacing, setSpacing] = useState(mm(2));
  const [rail, setRail] = useState(mm(5));
  const [vscore, setVscore] = useState(false);
  const [fiducials, setFiducials] = useState(true);
  const run = async () => {
    try {
      const s = await api.pcbPanelize(docId, { rows: Number(rows) || 1, cols: Number(cols) || 1, spacing, rail, vscore, fiducials });
      useEditor.getState().loadDoc(s);
      bus.emit("showDoc", { docId: s.info.id });
      useEditor.getState().log("info", "Panel created as a new board — save it, then generate fab outputs from it.");
      close();
    } catch (e) {
      report(e);
    }
  };
  return (
    <Dialog
      title="Panelize"
      description="Creates a new board with an array of this board, rails and optional V-score lines."
      footer={
        <>
          <Button variant="outline" onClick={close}>
            Cancel
          </Button>
          <Button onClick={() => void run()}>Create panel</Button>
        </>
      }
    >
      <div className="grid grid-cols-2 gap-3">
        <Field label="Rows">
          <Input className="font-mono" value={rows} onChange={(e) => setRows(e.target.value)} />
        </Field>
        <Field label="Columns">
          <Input className="font-mono" value={cols} onChange={(e) => setCols(e.target.value)} />
        </Field>
        <Field label="Spacing (routed)">
          <LengthInput value={spacing} units={units} onCommit={setSpacing} />
        </Field>
        <Field label="Rail width">
          <LengthInput value={rail} units={units} onCommit={setRail} />
        </Field>
        <label className="col-span-2 flex items-center gap-2">
          <Checkbox checked={vscore} onCheckedChange={(v) => setVscore(v === true)} /> V-score between boards (no spacing; score lines on Mechanical 1)
        </label>
        <label className="col-span-2 flex items-center gap-2">
          <Checkbox checked={fiducials} onCheckedChange={(v) => setFiducials(v === true)} /> Fiducials on the rails
        </label>
      </div>
    </Dialog>
  );
}

// ---------- Variants ----------

function VariantsDialog() {
  const project = useEditor((s) => s.project);
  const [variants, setVariants] = useState(project?.variants ?? []);
  const [current, setCurrent] = useState(0);
  const save = async () => {
    try {
      const p = await api.projectSetVariants(variants);
      useEditor.getState().setProject(p.project);
      close();
    } catch (e) {
      report(e);
    }
  };
  const v = variants[current];
  return (
    <Dialog
      title="Assembly variants"
      description="List the designators that are not fitted in each build. Choose a variant when generating the BOM and pick-and-place."
      footer={
        <>
          <Button variant="outline" onClick={close}>
            Cancel
          </Button>
          <Button onClick={() => void save()}>Save</Button>
        </>
      }
    >
      <div className="grid grid-cols-[160px_1fr] gap-3">
        <div className="grid content-start gap-1">
          {variants.map((x, i) => (
            <button key={i} onClick={() => setCurrent(i)} className={cn("rounded-[4px] px-2 py-1 text-left hover:bg-hover", i === current && "bg-sel")}>
              {x.name}
            </button>
          ))}
          <Button variant="outline" size="sm" onClick={() => (setVariants([...variants, { name: `Variant ${variants.length + 1}`, dnp: [] }]), setCurrent(variants.length))}>
            <Plus /> Variant
          </Button>
        </div>
        {v ? (
          <div className="grid content-start gap-2">
            <Field label="Name">
              <Input value={v.name} onChange={(e) => setVariants(variants.map((x, i) => (i === current ? { ...x, name: e.target.value } : x)))} />
            </Field>
            <Field label="Not fitted (designators, comma or space separated)">
              <Textarea
                className="min-h-28 font-mono text-xs"
                value={v.dnp.join(", ")}
                onChange={(e) => setVariants(variants.map((x, i) => (i === current ? { ...x, dnp: e.target.value.split(/[\s,;]+/).filter(Boolean) } : x)))}
              />
            </Field>
            <Button variant="ghost" size="sm" className="w-fit" onClick={() => (setVariants(variants.filter((_, i) => i !== current)), setCurrent(0))}>
              <Trash2 /> Delete variant
            </Button>
          </div>
        ) : (
          <p className="text-muted-foreground">No variants yet.</p>
        )}
      </div>
    </Dialog>
  );
}

export { Label };
