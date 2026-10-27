import { BulkProperties } from "./BulkProperties";
import { useState, type ReactNode } from "react";
import { ViewPanel } from "./ViewPanel";
import { api } from "../core/api";
import { deleteSelection, execute, mirrorSelection, report, rotateSelection } from "../core/actions";
import { boardTabs, FOOTPRINT_TABS, L, layer } from "../core/layers";
import { useEditor, type DocState, type Placement } from "../core/store";
import type { Field, Footprint, Item, ItemId, ItemOf, LibSymbol, SheetEntry } from "../core/types";
import type { Units } from "../core/units";
import { Button } from "@/components/ui/button";
import { Check, CommitInput, LengthInput, NumberInput, Select } from "@/components/app/fields";
import { EmptyNote, Panel, PanelHeading, PropRow, Section } from "@/components/app/panel";
import { MousePointerClick, Plus, X } from "lucide-react";
import { toolButton } from "../editor/profiles";
import { useLibrary } from "../library/LibraryView";
import { BoardItemEditor, boardPlacementRows } from "./BoardProperties";

// ---------- inputs ----------

const Row = PropRow;

const ROTATIONS: [number, string][] = [
  [0, "0°"],
  [90, "90°"],
  [180, "180°"],
  [270, "270°"],
];
const PIN_TYPES: [string, string][] = [
  ["input", "Input"],
  ["output", "Output"],
  ["bidirectional", "Bidirectional"],
  ["triState", "Tri-state"],
  ["passive", "Passive"],
  ["powerIn", "Power input"],
  ["powerOut", "Power output"],
  ["openCollector", "Open collector"],
  ["openEmitter", "Open emitter"],
  ["noConnect", "Not connected"],
  ["unspecified", "Unspecified"],
];
const PIN_SHAPES: [string, string][] = [
  ["line", "Line"],
  ["inverted", "Inverted (bubble)"],
  ["clock", "Clock"],
  ["invertedClock", "Inverted clock"],
];
const PORT_SHAPES: [string, string][] = [
  ["passive", "Passive"],
  ["input", "Input"],
  ["output", "Output"],
  ["bidirectional", "Bidirectional"],
  ["triState", "Tri-state"],
];
const POWER_STYLES: [string, string][] = [
  ["bar", "Bar"],
  ["arrow", "Arrow"],
  ["circle", "Circle"],
  ["wave", "Wave"],
  ["ground", "Ground"],
  ["signalGround", "Signal ground"],
  ["earth", "Earth"],
];
const PAD_SHAPES: [string, string][] = [
  ["roundRect", "Rounded rectangle"],
  ["rect", "Rectangle"],
  ["circle", "Circle"],
  ["oval", "Oval"],
];
const PAD_KINDS: [string, string][] = [
  ["smd", "SMD"],
  ["tht", "Through-hole (plated)"],
  ["npth", "Hole (non-plated)"],
];
const FILLS: [string, string][] = [
  ["none", "None"],
  ["solid", "Solid"],
  ["background", "Body color"],
];
const SCOPES: [string, string][] = [
  ["local", "Local (this sheet)"],
  ["global", "Global (all sheets)"],
  ["hierarchical", "Port (to parent sheet)"],
];
const H_ALIGN: [string, string][] = [
  ["left", "Left"],
  ["center", "Center"],
  ["right", "Right"],
];

// ---------- item editors ----------

function ItemEditor({ doc, id, item, units }: { doc: DocState; id: ItemId; item: Item; units: Units }) {
  const put = (next: Item) => void execute(doc.info.id, { op: "replace", id, item: next });
  const L_ = (v: number, f: (n: number) => void, min?: number) => <LengthInput value={v} units={units} onCommit={f} min={min} />;
  const board = doc.info.kind === "pcb" || doc.info.kind === "footprint";
  const layerOptions: [number, string][] = doc.info.kind === "footprint" ? FOOTPRINT_TABS.map((l) => [l, layer(l).name]) : boardTabs(doc.items.values()).map((t) => [t.id, t.name]);
  const layerRow = (it: { layer: number }) =>
    board && (
      <Row label="Layer">
        <Select value={it.layer} options={layerOptions} onChange={(v) => put({ ...item, layer: v } as Item)} />
      </Row>
    );

  switch (item.kind) {
    case "line":
      return (
        <Section title="Line">
          {layerRow(item)}
          <Row label="Width">{L_(item.width, (width) => put({ ...item, width }), 0)}</Row>
          <Row label="Start X">{L_(item.a.x, (x) => put({ ...item, a: { ...item.a, x } }))}</Row>
          <Row label="Start Y">{L_(item.a.y, (y) => put({ ...item, a: { ...item.a, y } }))}</Row>
          <Row label="End X">{L_(item.b.x, (x) => put({ ...item, b: { ...item.b, x } }))}</Row>
          <Row label="End Y">{L_(item.b.y, (y) => put({ ...item, b: { ...item.b, y } }))}</Row>
        </Section>
      );
    case "arc":
      return (
        <Section title="Arc">
          {layerRow(item)}
          <Row label="Width">{L_(item.width, (width) => put({ ...item, width }), 0)}</Row>
          <Row label="Center X">{L_(item.center.x, (x) => put({ ...item, center: { ...item.center, x } }))}</Row>
          <Row label="Center Y">{L_(item.center.y, (y) => put({ ...item, center: { ...item.center, y } }))}</Row>
          <Row label="Radius">{L_(item.radius, (radius) => put({ ...item, radius }), 1)}</Row>
          <Row label="Start angle">
            <NumberInput value={item.startAngle} onCommit={(startAngle) => put({ ...item, startAngle })} />
          </Row>
          <Row label="Sweep">
            <NumberInput value={item.sweep} onCommit={(sweep) => put({ ...item, sweep })} />
          </Row>
        </Section>
      );
    case "circle":
    case "rect":
    case "polyline":
      return (
        <Section title={item.kind === "circle" ? "Circle" : item.kind === "rect" ? "Rectangle" : "Polyline"}>
          {layerRow(item)}
          <Row label="Width">{L_(item.width, (width) => put({ ...item, width } as Item), 0)}</Row>
          <Row label="Fill">
            <Select value={item.fill} options={FILLS as [typeof item.fill, string][]} onChange={(fill) => put({ ...item, fill } as Item)} />
          </Row>
          {item.kind === "circle" && <Row label="Radius">{L_(item.radius, (radius) => put({ ...item, radius }), 1)}</Row>}
          {item.kind === "polyline" && (
            <Row label="Closed">
              <Check value={item.closed} onChange={(closed) => put({ ...item, closed })} />
            </Row>
          )}
        </Section>
      );
    case "text":
      return (
        <Section title="Text">
          {layerRow(item)}
          <Row label="Text">
            <CommitInput value={item.text} onCommit={(text) => text && put({ ...item, text })} />
          </Row>
          <Row label="Height">{L_(item.height, (height) => put({ ...item, height }), 1)}</Row>
          <Row label="Rotation">
            <NumberInput value={item.rotation} onCommit={(rotation) => put({ ...item, rotation })} />
          </Row>
          <Row label="Align">
            <Select value={item.hAlign} options={H_ALIGN as [typeof item.hAlign, string][]} onChange={(hAlign) => put({ ...item, hAlign })} />
          </Row>
          {board && (
            <Row label="Mirrored">
              <Check value={item.mirrored} onChange={(mirrored) => put({ ...item, mirrored })} />
            </Row>
          )}
        </Section>
      );
    case "pin":
      return (
        <Section title="Pin">
          <Row label="Name">
            <CommitInput value={item.name} onCommit={(name) => put({ ...item, name })} />
          </Row>
          <Row label="Number">
            <CommitInput value={item.number} onCommit={(number) => put({ ...item, number })} />
          </Row>
          <Row label="Type">
            <Select value={item.electrical} options={PIN_TYPES as [typeof item.electrical, string][]} onChange={(electrical) => put({ ...item, electrical })} />
          </Row>
          <Row label="Shape">
            <Select value={item.shape} options={PIN_SHAPES as [typeof item.shape, string][]} onChange={(shape) => put({ ...item, shape })} />
          </Row>
          <Row label="Length">{L_(item.length, (length) => put({ ...item, length }), 0)}</Row>
          <Row label="Direction">
            <Select value={item.rotation} options={ROTATIONS} onChange={(rotation) => put({ ...item, rotation })} />
          </Row>
          <Row label="Hidden">
            <Check value={item.hidden} onChange={(hidden) => put({ ...item, hidden })} />
          </Row>
          <Row label="X">{L_(item.pos.x, (x) => put({ ...item, pos: { ...item.pos, x } }))}</Row>
          <Row label="Y">{L_(item.pos.y, (y) => put({ ...item, pos: { ...item.pos, y } }))}</Row>
        </Section>
      );
    case "pad":
      return (
        <Section title="Pad">
          <Row label="Number">
            <CommitInput value={item.number} onCommit={(number) => put({ ...item, number })} />
          </Row>
          <Row label="Type">
            <Select
              value={item.padKind}
              options={PAD_KINDS as [typeof item.padKind, string][]}
              onChange={(padKind) =>
                put({
                  ...item,
                  padKind,
                  layer: padKind === "smd" ? (item.layer === L.multi ? L.topCopper : item.layer) : L.multi,
                  drill: padKind === "smd" ? null : (item.drill ?? { width: Math.round(Math.min(item.width, item.height) * 0.6), height: Math.round(Math.min(item.width, item.height) * 0.6) }),
                })
              }
            />
          </Row>
          {item.padKind === "smd" && (
            <Row label="Side">
              <Select value={item.layer} options={[[L.topCopper, "Top"], [L.bottomCopper, "Bottom"]]} onChange={(layer) => put({ ...item, layer })} />
            </Row>
          )}
          <Row label="Shape">
            <Select value={item.shape} options={PAD_SHAPES as [typeof item.shape, string][]} onChange={(shape) => put({ ...item, shape, cornerRatio: shape === "roundRect" ? item.cornerRatio || 0.25 : item.cornerRatio })} />
          </Row>
          <Row label="Width">{L_(item.width, (width) => put({ ...item, width }), 1)}</Row>
          <Row label="Height">{L_(item.height, (height) => put({ ...item, height }), 1)}</Row>
          {item.shape === "roundRect" && (
            <Row label="Corner ratio">
              <NumberInput value={item.cornerRatio} onCommit={(cornerRatio) => put({ ...item, cornerRatio: Math.min(0.5, Math.max(0, cornerRatio)) })} />
            </Row>
          )}
          {item.drill && (
            <>
              <Row label="Drill">{L_(item.drill.width, (w) => put({ ...item, drill: { width: w, height: item.drill!.height === item.drill!.width ? w : item.drill!.height } }), 1)}</Row>
              <Row label="Slot length">{L_(item.drill.height, (h) => put({ ...item, drill: { ...item.drill!, height: h } }), 1)}</Row>
            </>
          )}
          <Row label="Rotation">
            <NumberInput value={item.rotation} onCommit={(rotation) => put({ ...item, rotation })} />
          </Row>
          <Row label="X">{L_(item.pos.x, (x) => put({ ...item, pos: { ...item.pos, x } }))}</Row>
          <Row label="Y">{L_(item.pos.y, (y) => put({ ...item, pos: { ...item.pos, y } }))}</Row>
        </Section>
      );
    case "netLabel":
      return (
        <Section title="Net Label">
          <Row label="Net">
            <CommitInput value={item.text} onCommit={(text) => text && put({ ...item, text })} />
          </Row>
          <Row label="Scope">
            <Select value={item.scope} options={SCOPES as [typeof item.scope, string][]} onChange={(scope) => put({ ...item, scope })} />
          </Row>
          {item.scope !== "local" && (
            <Row label="Shape">
              <Select value={item.shape} options={PORT_SHAPES as [typeof item.shape, string][]} onChange={(shape) => put({ ...item, shape })} />
            </Row>
          )}
          <Row label="Rotation">
            <Select value={item.rotation} options={ROTATIONS} onChange={(rotation) => put({ ...item, rotation })} />
          </Row>
        </Section>
      );
    case "powerPort":
      return (
        <Section title="Power Port">
          <Row label="Net">
            <CommitInput value={item.net} onCommit={(net) => net && put({ ...item, net })} />
          </Row>
          <Row label="Style">
            <Select value={item.style} options={POWER_STYLES as [typeof item.style, string][]} onChange={(style) => put({ ...item, style })} />
          </Row>
          <Row label="Orientation">
            <Select
              value={item.rotation}
              options={[
                [90, "Up"],
                [270, "Down"],
                [0, "Right"],
                [180, "Left"],
              ]}
              onChange={(rotation) => put({ ...item, rotation })}
            />
          </Row>
        </Section>
      );
    case "footprint":
    case "track":
    case "arcTrack":
    case "via":
    case "zone":
    case "dimension":
      return <BoardItemEditor doc={doc} id={id} item={item} put={put} units={units} />;
    case "part":
      return <PartEditor doc={doc} id={id} part={item} put={put} units={units} />;
    case "sheet":
      return <SheetEditor sheet={item} put={put} units={units} />;
    case "wire":
    case "bus":
      return (
        <Section title={item.kind === "wire" ? "Wire" : "Bus"}>
          <Row label="Start X">{L_(item.a.x, (x) => put({ ...item, a: { ...item.a, x } }))}</Row>
          <Row label="Start Y">{L_(item.a.y, (y) => put({ ...item, a: { ...item.a, y } }))}</Row>
          <Row label="End X">{L_(item.b.x, (x) => put({ ...item, b: { ...item.b, x } }))}</Row>
          <Row label="End Y">{L_(item.b.y, (y) => put({ ...item, b: { ...item.b, y } }))}</Row>
        </Section>
      );
    default:
      return (
        <Section title={item.kind}>
          <Row label="X">{L_((item as { pos: { x: number } }).pos.x, (x) => put({ ...item, pos: { ...(item as { pos: { x: number; y: number } }).pos, x } } as Item))}</Row>
          <Row label="Y">{L_((item as { pos: { y: number } }).pos.y, (y) => put({ ...item, pos: { ...(item as { pos: { x: number; y: number } }).pos, y } } as Item))}</Row>
        </Section>
      );
  }
}

function PartEditor({ doc, id, part, put, units }: { doc: DocState; id: ItemId; part: ItemOf<"part">; put: (i: Item) => void; units: Units }) {
  const cached = doc.cache[part.libRef];
  const setField = (name: string, patch: Partial<Field>) => {
    const exists = part.fields.some((f) => f.name === name);
    const fields = exists
      ? part.fields.map((f) => (f.name === name ? { ...f, ...patch } : f))
      : [...part.fields, { name, value: "", offset: { x: 0, y: 0 }, rotation: 0, height: 1_270_000, hAlign: "left", vAlign: "middle", visible: false, ...patch } as Field];
    put({ ...part, fields });
  };
  const value = (n: string) => part.fields.find((f) => f.name === n)?.value ?? "";
  const unitCount = cached?.symbol.units.length ?? 1;
  const footprints = cached?.component.footprints ?? [];

  return (
    <>
      <Section title="Part">
        <Row label="Designator">
          <CommitInput value={value("Designator")} onCommit={(v) => setField("Designator", { value: v, visible: true })} />
        </Row>
        <Row label="Value">
          <CommitInput value={value("Value")} onCommit={(v) => setField("Value", { value: v })} />
        </Row>
        <Row label="Footprint">
          {footprints.length > 1 ? (
            <Select value={value("Footprint")} options={footprints.map((f) => [f, f] as [string, string])} onChange={(v) => setField("Footprint", { value: v })} />
          ) : (
            <CommitInput value={value("Footprint")} onCommit={(v) => setField("Footprint", { value: v })} />
          )}
        </Row>
        {unitCount > 1 && (
          <Row label="Unit">
            <Select value={part.unit} options={Array.from({ length: unitCount }, (_, i) => [i + 1, String.fromCharCode(65 + i)] as [number, string])} onChange={(unit) => put({ ...part, unit })} />
          </Row>
        )}
        <Row label="Rotation">
          <Select value={part.rotation} options={ROTATIONS} onChange={(rotation) => void execute(doc.info.id, { op: "rotate", ids: [id], center: part.pos, angle: rotation - part.rotation })} />
        </Row>
        <Row label="Mirrored">
          <Check value={part.mirror} onChange={() => void execute(doc.info.id, { op: "mirror", ids: [id], center: part.pos })} />
        </Row>
        <Row label="X">
          <LengthInput value={part.pos.x} units={units} onCommit={(x) => put({ ...part, pos: { ...part.pos, x } })} />
        </Row>
        <Row label="Y">
          <LengthInput value={part.pos.y} units={units} onCommit={(y) => put({ ...part, pos: { ...part.pos, y } })} />
        </Row>
      </Section>
      <Section title="Fields">
        {part.fields.map((f) => (
          <Row key={f.name} label={f.name}>
            <div className="flex items-center gap-2">
              <CommitInput value={f.value} onCommit={(v) => setField(f.name, { value: v })} />
              <Check value={f.visible} onChange={(visible) => setField(f.name, { visible })} />
            </div>
          </Row>
        ))}
      </Section>
      {cached && (
        <Section title="Library">
          <div className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 text-[11px] [&>span:nth-child(odd)]:text-muted-foreground [&>span:nth-child(even)]:truncate [&>span:nth-child(even)]:font-mono">
            <span>Component</span>
            <span>{cached.component.name}</span>
            <span>Library</span>
            <span>{cached.library}</span>
            {cached.component.parameters
              .filter((p) => p.name !== "Value" && p.value)
              .slice(0, 8)
              .flatMap((p) => [<span key={`${p.name}-k`}>{p.name}</span>, <span key={`${p.name}-v`}>{p.value}</span>])}
          </div>
        </Section>
      )}
    </>
  );
}


function SheetEditor({ sheet, put, units }: { sheet: ItemOf<"sheet">; put: (i: Item) => void; units: Units }) {
  const setEntry = (i: number, patch: Partial<SheetEntry>) => put({ ...sheet, entries: sheet.entries.map((e, j) => (j === i ? { ...e, ...patch } : e)) });
  const addEntry = () => {
    const step = 2_540_000;
    const left = sheet.entries.filter((e) => e.side === "left").length;
    put({ ...sheet, entries: [...sheet.entries, { name: `IN${sheet.entries.length + 1}`, side: "left", offset: step * (left + 1), shape: "input" }] });
  };
  return (
    <>
      <Section title="Sheet Symbol">
        <Row label="Name">
          <CommitInput value={sheet.name} onCommit={(name) => put({ ...sheet, name })} />
        </Row>
        <Row label="File">
          <CommitInput value={sheet.file} onCommit={(file) => file && put({ ...sheet, file })} />
        </Row>
        <Row label="Width">
          <LengthInput value={sheet.size.x} units={units} min={1} onCommit={(x) => put({ ...sheet, size: { ...sheet.size, x } })} />
        </Row>
        <Row label="Height">
          <LengthInput value={sheet.size.y} units={units} min={1} onCommit={(y) => put({ ...sheet, size: { ...sheet.size, y } })} />
        </Row>
      </Section>
      <Section
        title="Entries"
        actions={
          <Button variant="ghost" size="icon-xs" onClick={addEntry} title="Add entry"><Plus /></Button>
        }
      >
        {sheet.entries.map((e, i) => (
          <div key={i} className="grid grid-cols-[1fr_52px_96px_80px_24px] gap-1 py-0.5">
            <CommitInput value={e.name} onCommit={(name) => setEntry(i, { name })} />
            <Select value={e.side} options={[["left", "L"], ["right", "R"], ["top", "T"], ["bottom", "B"]]} onChange={(side) => setEntry(i, { side })} />
            <Select value={e.shape} options={PORT_SHAPES as [typeof e.shape, string][]} onChange={(shape) => setEntry(i, { shape })} />
            <LengthInput value={e.offset} units={units} onCommit={(offset) => setEntry(i, { offset })} />
            <Button variant="ghost" size="icon-xs" onClick={() => put({ ...sheet, entries: sheet.entries.filter((_, j) => j !== i) })}><X /></Button>
          </div>
        ))}
        {!sheet.entries.length && <p className="text-[11px] text-muted-foreground">Entries connect to Port labels with the same name inside {sheet.file}.</p>}
      </Section>
    </>
  );
}

// ---------- placement / document ----------

function PlacementEditor({ tool, kind, units }: { tool: string; kind: string; units: Units }) {
  const pl = useEditor((s) => s.placement);
  const set = (p: Partial<Placement>) => useEditor.getState().setPlacement(p);
  const L_ = (v: number, f: (n: number) => void, min?: number) => <LengthInput value={v} units={units} onCommit={f} min={min} />;
  const rows: ReactNode[] = [];
  const impl = toolButton(kind as never, tool)?.tool ?? tool;
  const board = kind === "pcb" || kind === "footprint";
  const doc = useEditor((s) => (s.activeDocId ? s.docs[s.activeDocId] : undefined));
  const boardRows = kind === "pcb" ? boardPlacementRows(impl, pl, set, units, doc) : null;
  if (boardRows) return <Section title={`Placing: ${toolButton(kind as never, tool)?.label ?? tool}`}>{boardRows}</Section>;

  if (impl === "label" || impl === "globalLabel" || impl === "port") {
    rows.push(
      <Row key="t" label="Net name">
        <CommitInput value={pl.labelText} onCommit={(labelText) => set({ labelText })} />
      </Row>,
    );
    if (impl !== "label")
      rows.push(
        <Row key="s" label="Shape">
          <Select value={pl.labelShape} options={PORT_SHAPES as [typeof pl.labelShape, string][]} onChange={(labelShape) => set({ labelShape })} />
        </Row>,
      );
  }
  if (impl === "power") {
    rows.push(
      <Row key="n" label="Net">
        <CommitInput value={pl.powerNet} onCommit={(powerNet) => set({ powerNet })} />
      </Row>,
      <Row key="s" label="Style">
        <Select value={pl.powerStyle} options={POWER_STYLES as [typeof pl.powerStyle, string][]} onChange={(powerStyle) => set({ powerStyle })} />
      </Row>,
    );
  }
  if (impl === "pin") {
    rows.push(
      <Row key="n" label="Name">
        <CommitInput value={pl.pinName} onCommit={(pinName) => set({ pinName })} />
      </Row>,
      <Row key="#" label="Number">
        <CommitInput value={pl.pinNumber} onCommit={(pinNumber) => set({ pinNumber })} />
      </Row>,
      <Row key="t" label="Type">
        <Select value={pl.pinType} options={PIN_TYPES as [typeof pl.pinType, string][]} onChange={(pinType) => set({ pinType })} />
      </Row>,
      <Row key="s" label="Shape">
        <Select value={pl.pinShape} options={PIN_SHAPES as [typeof pl.pinShape, string][]} onChange={(pinShape) => set({ pinShape })} />
      </Row>,
      <Row key="l" label="Length">{L_(pl.pinLength, (pinLength) => set({ pinLength }), 0)}</Row>,
    );
  }
  if (impl === "pad") {
    rows.push(
      <Row key="#" label="Number">
        <CommitInput value={pl.padNumber} onCommit={(padNumber) => set({ padNumber })} />
      </Row>,
      <Row key="k" label="Type">
        <Select value={pl.padKind} options={PAD_KINDS as [typeof pl.padKind, string][]} onChange={(padKind) => set({ padKind })} />
      </Row>,
      <Row key="s" label="Shape">
        <Select value={pl.padShape} options={PAD_SHAPES as [typeof pl.padShape, string][]} onChange={(padShape) => set({ padShape })} />
      </Row>,
      <Row key="w" label="Width">{L_(pl.padWidth, (padWidth) => set({ padWidth }), 1)}</Row>,
      <Row key="h" label="Height">{L_(pl.padHeight, (padHeight) => set({ padHeight }), 1)}</Row>,
    );
    if (pl.padKind !== "smd") rows.push(<Row key="d" label="Drill">{L_(pl.padDrill, (padDrill) => set({ padDrill }), 1)}</Row>);
  }
  if (impl === "text") {
    rows.push(
      <Row key="t" label="Text">
        <CommitInput value={pl.text} onCommit={(text) => set({ text })} />
      </Row>,
      <Row key="h" label="Height">{L_(pl.textHeight, (textHeight) => set({ textHeight }), 1)}</Row>,
    );
  }
  if (impl === "sheet") {
    rows.push(
      <Row key="n" label="Sheet name">
        <CommitInput value={pl.sheetName} onCommit={(sheetName) => set({ sheetName })} />
      </Row>,
    );
  }
  if (["rect", "circle"].includes(impl)) {
    rows.push(
      <Row key="f" label="Fill">
        <Select value={pl.fill} options={FILLS as [typeof pl.fill, string][]} onChange={(fill) => set({ fill })} />
      </Row>,
    );
  }
  if (board && ["line", "rect", "circle", "arc", "polyline"].includes(impl)) {
    rows.push(<Row key="w" label="Line width">{L_(pl.lineWidth, (lineWidth) => set({ lineWidth }), 1)}</Row>);
  }
  if (impl === "part" && pl.part) {
    rows.push(
      <Row key="c" label="Component">
        <span className="font-mono text-ink">{pl.part.component}</span>
      </Row>,
      <Row key="u" label="Next unit">
        <span className="font-mono text-ink">{String.fromCharCode(64 + pl.part.unit)}</span>
      </Row>,
    );
  }
  rows.push(
    <Row key="r" label="Rotation">
      <Select value={pl.rotation} options={ROTATIONS} onChange={(rotation) => set({ rotation })} />
    </Row>,
  );

  return <Section title={`Placing: ${toolButton(kind as never, tool)?.label ?? tool}`}>{rows}</Section>;
}

function SymbolMetaEditor({ libId, name }: { libId: string; name: string }) {
  const lib = useLibrary(libId);
  const sym = lib?.symbols.find((s) => s.name === name);
  const units = useEditor((s) => s.units);
  if (!sym) return null;
  const update = async (patch: Partial<LibSymbol>) => {
    const next = { ...sym, ...patch };
    try {
      await api.updateSymbol(libId, name, {
        name: next.name,
        description: next.description,
        prefix: next.prefix,
        showPinNumbers: next.showPinNumbers,
        showPinNames: next.showPinNames,
        pinNameOffset: next.pinNameOffset,
        isPower: next.isPower,
        fields: next.fields,
      });
      useEditor.getState().bumpLib(libId);
    } catch (e) {
      report(e);
    }
  };
  const field = (n: string) => sym.fields.find((f) => f.name === n);
  const setFieldOffset = (n: string, axis: "x" | "y", v: number) => {
    const f = field(n);
    const base: Field = f ?? { name: n, value: "", offset: { x: 0, y: 0 }, rotation: 0, height: 1_270_000, hAlign: "left", vAlign: "middle", visible: true };
    const fields = [...sym.fields.filter((x) => x.name !== n), { ...base, offset: { ...base.offset, [axis]: v } }];
    void update({ fields });
  };
  return (
    <>
      <Section title="Symbol">
        <Row label="Name">
          <CommitInput value={sym.name} onCommit={(v) => v && void update({ name: v })} />
        </Row>
        <Row label="Prefix">
          <CommitInput value={sym.prefix} onCommit={(v) => void update({ prefix: v })} />
        </Row>
        <Row label="Description">
          <CommitInput value={sym.description} onCommit={(v) => void update({ description: v })} />
        </Row>
        <Row label="Pin numbers">
          <Check value={sym.showPinNumbers} onChange={(showPinNumbers) => void update({ showPinNumbers })} />
        </Row>
        <Row label="Pin names">
          <Check value={sym.showPinNames} onChange={(showPinNames) => void update({ showPinNames })} />
        </Row>
        <Row label="Name offset">
          <LengthInput value={sym.pinNameOffset} units={units} min={0} onCommit={(pinNameOffset) => void update({ pinNameOffset })} />
        </Row>
        <Row label="Power symbol">
          <Check value={sym.isPower} onChange={(isPower) => void update({ isPower })} />
        </Row>
      </Section>
      <Section title="Field placement">
        {["Designator", "Value"].map((n) => (
          <div key={n}>
            <Row label={`${n} X`}>
              <LengthInput value={field(n)?.offset.x ?? 0} units={units} onCommit={(v) => setFieldOffset(n, "x", v)} />
            </Row>
            <Row label={`${n} Y`}>
              <LengthInput value={field(n)?.offset.y ?? 0} units={units} onCommit={(v) => setFieldOffset(n, "y", v)} />
            </Row>
          </div>
        ))}
      </Section>
    </>
  );
}

function FootprintMetaEditor({ libId, name }: { libId: string; name: string }) {
  const lib = useLibrary(libId);
  const fp: Footprint | undefined = lib?.footprints.find((f) => f.name === name);
  if (!fp) return null;
  const update = async (patch: Partial<Footprint>) => {
    const next = { ...fp, ...patch };
    try {
      await api.updateFootprint(libId, name, { name: next.name, description: next.description, model: next.model });
      useEditor.getState().bumpLib(libId);
    } catch (e) {
      report(e);
    }
  };
  return (
    <Section title="Footprint">
      <Row label="Name">
        <CommitInput value={fp.name} onCommit={(v) => v && void update({ name: v })} />
      </Row>
      <Row label="Description">
        <CommitInput value={fp.description} onCommit={(v) => void update({ description: v })} />
      </Row>
      <Row label="3D model">
        <CommitInput
          value={fp.model?.path ?? ""}
          onCommit={(v) => void update({ model: v ? { path: v, offset: fp.model?.offset ?? [0, 0, 0], rotation: fp.model?.rotation ?? [0, 0, 0], scale: fp.model?.scale ?? [1, 1, 1] } : null })}
        />
      </Row>
      <p className="text-[11px] text-muted-foreground">3D models are shown in the 3D viewer (Phase 4).</p>
    </Section>
  );
}

function DocumentOptions({ doc }: { doc: DocState }) {
  const units = useEditor((s) => s.units);
  return (
    <>
      {doc.info.kind === "symbol" && doc.info.library && doc.info.entry && <SymbolMetaEditor libId={doc.info.library} name={doc.info.entry} />}
      {doc.info.kind === "footprint" && doc.info.library && doc.info.entry && <FootprintMetaEditor libId={doc.info.library} name={doc.info.entry} />}
      <Section title="Document Options">
        <Row label="Units">
          <Select value={units} options={[["mm", "Metric (mm)"], ["mil", "Imperial (mil)"]]} onChange={(u) => u !== units && useEditor.getState().toggleUnits()} />
        </Row>
        <Row label="Snap to grid">
          <Check value={doc.view.snapToGrid} onChange={(snapToGrid) => useEditor.getState().setView(doc.info.id, { snapToGrid })} />
        </Row>
        <Row label="Snap grid">
          <LengthInput value={doc.grid} units={units} min={1000} onCommit={(grid) => useEditor.getState().patchDoc(doc.info.id, { grid })} />
        </Row>
        <Row label="Objects">
          <span className="font-mono text-ink">{doc.items.size}</span>
        </Row>
      </Section>
    </>
  );
}

export function PropertiesPanel() {
  const [panelTab, setPanelTab] = useState<"properties" | "view">("properties");
  const doc = useEditor((s) => (s.activeDocId ? s.docs[s.activeDocId] : undefined));
  const units = useEditor((s) => s.units);
  if (!doc)
    return (
      <Panel>
        <PanelHeading title="Properties" />
        <EmptyNote icon={<MousePointerClick />} title="Nothing to show">
          Open a document and select an object to edit it here.
        </EmptyNote>
      </Panel>
    );

  const canView = doc.info.kind === "pcb" || doc.info.kind === "footprint";
  let body: ReactNode;
  if (canView && panelTab === "view") {
    body = <ViewPanel doc={doc} />;
  } else if (doc.tool !== "select") {
    body = <PlacementEditor tool={doc.tool} kind={doc.info.kind} units={units} />;
  } else if (doc.selection.size === 1) {
    const id = [...doc.selection][0];
    const item = doc.items.get(id);
    body = item ? <ItemEditor key={`item:${id}`} doc={doc} id={id} item={item} units={units} /> : null;
  } else if (doc.selection.size > 1) {
    const kinds = new Map<string, number>();
    for (const id of doc.selection) {
      const k = doc.items.get(id)?.kind ?? "?";
      kinds.set(k, (kinds.get(k) ?? 0) + 1);
    }
    body = (
      <Section title={`${doc.selection.size} objects selected`}>
        <div className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 text-[11px] [&>span:nth-child(odd)]:text-muted-foreground [&>span:nth-child(even)]:truncate [&>span:nth-child(even)]:font-mono">
          {[...kinds].flatMap(([k, n]) => [<span key={`${k}k`}>{k}</span>, <span key={`${k}v`}>{n}</span>])}
        </div>
        <div className="flex flex-wrap gap-1.5 pt-2">
          <Button variant="outline" size="sm" onClick={() => void rotateSelection()}>
            Rotate
          </Button>
          <Button variant="outline" size="sm" onClick={() => void mirrorSelection()}>
            Mirror
          </Button>
          <Button variant="outline" size="sm" onClick={() => void deleteSelection()}>
            Delete
          </Button>
        </div>
      </Section>
    );
  } else {
    body = <DocumentOptions doc={doc} />;
  }
  const context =
    canView && panelTab === "view" ? "View" : doc.tool !== "select" ? "Placement" : doc.selection.size === 1 ? (doc.items.get([...doc.selection][0])?.kind ?? "") : doc.selection.size > 1 ? "Selection" : doc.info.title;
  return (
    <Panel>
      <PanelHeading title="Properties">
        <span className="max-w-40 truncate text-[11px] text-faint capitalize">{context}</span>
      </PanelHeading>
      {canView && <div className="flex gap-1 border-b border-line bg-panel-head px-2 py-1.5" role="tablist" aria-label="Panel mode">
        {(["properties", "view"] as const).map(tab => <button key={tab} role="tab" aria-selected={panelTab === tab} onClick={() => setPanelTab(tab)} className={`h-7 flex-1 rounded-[4px] text-xs capitalize ${panelTab === tab ? "bg-sel text-accent-ink" : "text-muted-foreground hover:bg-hover"}`}>{tab}</button>)}
      </div>}
      <div key={doc.info.id} className="min-h-0 flex-1 overflow-auto">
        {body}
        {doc.info.kind === "pcb" && doc.tool === "select" && doc.selection.size > 0 && panelTab === "properties" && (
          <BulkProperties key={`bulk:${[...doc.selection].sort().join(",")}`} doc={doc} />
        )}
      </div>
    </Panel>
  );
}
