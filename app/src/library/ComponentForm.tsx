import { useEffect, useMemo, useState } from "react";
import { MousePointerClick, Plus, Save, Star, X } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { NativeSelect, NativeSelectOption } from "@/components/ui/native-select";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { cn } from "@/lib/utils";
import { api } from "../core/api";
import { report } from "../core/actions";
import type { Component, Footprint, Library, PinItem, PinMapping } from "../core/types";
import { PreviewCanvas } from "../editor/PreviewCanvas";

const Row = ({ label, children }: { label: string; children: React.ReactNode }) => (
  <label className="grid grid-cols-[110px_minmax(0,1fr)] items-center gap-2 py-0.5">
    <span className="text-muted-foreground">{label}</span>
    <span className="min-w-0">{children}</span>
  </label>
);
const Heading = ({ children }: { children: React.ReactNode }) => (
  <div className="mt-4 mb-1.5 flex items-center gap-2 border-b border-line pb-1">
    <span className="eyebrow">{children}</span>
  </div>
);


/** Edits a component: symbol link, footprint options, parameters, pin map. */
export function ComponentForm({
  libId,
  lib,
  name,
  readonly,
  onSaved,
  onPlace,
}: {
  libId: string;
  lib: Library;
  name: string;
  readonly: boolean;
  onSaved: (name: string) => void;
  onPlace: () => void;
}) {
  const original = lib.components.find((c) => c.name === name);
  const [c, setC] = useState<Component | null>(original ? structuredClone(original) : null);
  const [fpIndex, setFpIndex] = useState(0);
  const [footprint, setFootprint] = useState<Footprint | null>(null);
  const [newFp, setNewFp] = useState("");

  const symbol = lib.symbols.find((s) => s.name === c?.symbol) ?? null;
  const pins = useMemo(
    () =>
      symbol?.units.flatMap((u, i) => u.items.filter((it): it is PinItem => it.kind === "pin").map((p) => ({ unit: i + 1, pin: p }))) ?? [],
    [symbol],
  );
  const dirty = JSON.stringify(c) !== JSON.stringify(original);

  const fpRef = c?.footprints[fpIndex];
  useEffect(() => {
    if (!fpRef) return setFootprint(null);
    const local = !fpRef.includes(":") ? lib.footprints.find((f) => f.name === fpRef) : undefined;
    if (local) return setFootprint(local);
    api.findFootprint(libId, fpRef).then(setFootprint).catch(() => setFootprint(null));
  }, [fpRef, lib, libId]);

  if (!c) return null;
  const set = (patch: Partial<Component>) => setC({ ...c, ...patch });
  const customMap = c.pinMap.length > 0;
  const padsFor = (unit: number, pin: string) => c.pinMap.find((m) => m.unit === unit && m.pin === pin)?.pads.join(", ") ?? "";

  const setPads = (unit: number, pin: string, text: string) => {
    const pads = text
      .split(/[,\s]+/)
      .map((s) => s.trim())
      .filter(Boolean);
    const others = c.pinMap.filter((m) => !(m.unit === unit && m.pin === pin));
    set({ pinMap: [...others, { unit, pin, pads }] });
  };

  const enableCustomMap = (on: boolean) => {
    if (!on) return set({ pinMap: [] });
    const map: PinMapping[] = pins.map(({ unit, pin }) => ({ unit, pin: pin.number, pads: [pin.number] }));
    set({ pinMap: map });
  };

  const padNumbers = new Set(footprint?.items.filter((i) => i.kind === "pad").map((p) => (p.kind === "pad" ? p.number : "")) ?? []);
  const unmatched = footprint
    ? pins.filter(({ unit, pin }) => {
        const pads = customMap ? (c.pinMap.find((m) => m.unit === unit && m.pin === pin.number)?.pads ?? []) : [pin.number];
        return !pads.length || pads.some((p) => !padNumbers.has(p));
      }).length
    : 0;

  const save = async () => {
    try {
      await api.putComponent(libId, c, name);
      onSaved(c.name);
    } catch (e) {
      report(e);
    }
  };

  return (
    <div className="text-xs">
      <div className="mb-3 flex items-center justify-between gap-3">
        <h3 className="font-mono text-[15px] font-semibold">{name}</h3>
        <div className="flex gap-1.5">
          <Button variant="outline" onClick={onPlace} title="Place on the active schematic">
            <MousePointerClick /> Place
          </Button>
          {!readonly && (
            <Button disabled={!dirty} onClick={() => void save()}>
              <Save /> Save component
            </Button>
          )}
        </div>
      </div>
      <fieldset disabled={readonly} className="min-w-0">
        <Row label="Name">
          <Input value={c.name} onChange={(e) => set({ name: e.target.value })} />
        </Row>
        <Row label="Description">
          <Input value={c.description} onChange={(e) => set({ description: e.target.value })} />
        </Row>
        <Row label="Symbol">
          <NativeSelect value={c.symbol} onChange={(e) => set({ symbol: e.target.value })}>
            {!symbol && <NativeSelectOption value={c.symbol}>{c.symbol} (missing)</NativeSelectOption>}
            {lib.symbols.map((s) => (
              <NativeSelectOption key={s.name} value={s.name}>
                {s.name}
              </NativeSelectOption>
            ))}
          </NativeSelect>
        </Row>

        <Heading>Footprints</Heading>
        <div className="mb-2 flex flex-wrap gap-1">
          {c.footprints.map((f, i) => (
            <span
              key={`${f}-${i}`}
              onClick={() => setFpIndex(i)}
              className={cn(
                "inline-flex cursor-default items-center gap-1 rounded-full border border-line bg-field py-0.5 pr-1 pl-2.5 font-mono text-[11px]",
                i === fpIndex && "border-accent-ink bg-sel",
              )}
            >
              {i === 0 && <span className="font-sans text-[10px] text-copper">default</span>}
              {f}
              {i > 0 && (
                <button className="rounded-full px-1 text-faint hover:text-ink" title="Make default" onClick={(e) => (e.stopPropagation(), set({ footprints: [f, ...c.footprints.filter((_, j) => j !== i)] }), setFpIndex(0))}>
                  <Star className="size-3" />
                </button>
              )}
              <button className="rounded-full px-1 text-faint hover:text-ink" title="Remove" onClick={(e) => (e.stopPropagation(), set({ footprints: c.footprints.filter((_, j) => j !== i) }), setFpIndex(0))}>
                <X className="size-3" />
              </button>
            </span>
          ))}
          {!c.footprints.length && <span className="text-muted-foreground">No footprint yet — the board won't know this part's pads.</span>}
        </div>
        <div className="flex gap-1.5">
          <Input list={`fp-${libId}`} placeholder="Footprint name or Library:Name" value={newFp} onChange={(e) => setNewFp(e.target.value)} className="font-mono" />
          <datalist id={`fp-${libId}`}>
            {lib.footprints.slice(0, 2000).map((f) => (
              <option key={f.name} value={f.name} />
            ))}
          </datalist>
          <Button variant="outline" disabled={!newFp.trim()} onClick={() => (set({ footprints: [...c.footprints, newFp.trim()] }), setNewFp(""))}>
            <Plus /> Add
          </Button>
        </div>

        <Heading>Parameters</Heading>
        <div className="grid gap-1">
          {c.parameters.map((p, i) => (
            <div key={i} className="grid grid-cols-[minmax(0,1fr)_minmax(0,1.4fr)_24px] gap-1">
              <Input value={p.name} placeholder="Name" onChange={(e) => set({ parameters: c.parameters.map((q, j) => (j === i ? { ...q, name: e.target.value } : q)) })} />
              <Input value={p.value} placeholder="Value" onChange={(e) => set({ parameters: c.parameters.map((q, j) => (j === i ? { ...q, value: e.target.value } : q)) })} />
              <Button variant="ghost" size="icon" onClick={() => set({ parameters: c.parameters.filter((_, j) => j !== i) })}>
                <X />
              </Button>
            </div>
          ))}
        </div>
        <Button variant="outline" size="sm" className="mt-1.5" onClick={() => set({ parameters: [...c.parameters, { name: "", value: "" }] })}>
          <Plus /> Parameter
        </Button>

        <div className="mt-4 mb-1.5 flex items-center gap-3 border-b border-line pb-1">
          <span className="eyebrow">Pin → pad mapping</span>
          <label className="flex items-center gap-1.5 text-muted-foreground">
            <Checkbox className="size-3.5" checked={customMap} onCheckedChange={(v) => enableCustomMap(v === true)} /> custom
          </label>
          {footprint &&
            (unmatched ? (
              <Badge variant="destructive" className="ml-auto">
                {unmatched} pins don't match pads
              </Badge>
            ) : (
              <Badge variant="secondary" className="ml-auto text-ok">
                all pins match pads
              </Badge>
            ))}
        </div>
        {customMap ? (
          <Table className="text-xs">
            <TableHeader>
              <TableRow>
                <TableHead className="h-7">Unit</TableHead>
                <TableHead className="h-7">Pin</TableHead>
                <TableHead className="h-7">Name</TableHead>
                <TableHead className="h-7">Pads</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {pins.map(({ unit, pin }) => (
                <TableRow key={`${unit}-${pin.number}`}>
                  <TableCell className="py-1 font-mono">{String.fromCharCode(64 + unit)}</TableCell>
                  <TableCell className="py-1 font-mono">{pin.number}</TableCell>
                  <TableCell className="py-1">{pin.name}</TableCell>
                  <TableCell className="py-1">
                    <Input defaultValue={padsFor(unit, pin.number)} onBlur={(e) => setPads(unit, pin.number, e.target.value)} className="h-6 font-mono" />
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        ) : (
          <p className="text-muted-foreground">Pins connect to the pads with the same number.</p>
        )}
      </fieldset>

      <div className="mt-4 grid grid-cols-1 gap-3 xl:grid-cols-2">
        <div>
          <div className="eyebrow mb-1">Symbol</div>
          <PreviewCanvas items={symbol?.units[0]?.items ?? []} kind="symbol" symbol={symbol} height={230} />
        </div>
        <div>
          <div className="eyebrow mb-1">Footprint {fpRef ? `· ${fpRef}` : ""}</div>
          <PreviewCanvas items={footprint?.items ?? []} kind="footprint" height={230} />
        </div>
      </div>
    </div>
  );
}
