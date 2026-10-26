// Properties editors for board objects and board placement tools.

import { useEffect, useId, useState, type ReactNode } from "react";
import { RefreshCw, Scissors } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Check, CommitInput, LengthInput, NumberInput, Select } from "@/components/app/fields";
import { PropRow as Row, Section } from "@/components/app/panel";
import { api } from "../core/api";
import { bus } from "../core/bus";
import { layer } from "../core/layers";
import { highlightNetOnBoard, repour } from "../core/pcbActions";
import { useEditor, type DocState, type Placement } from "../core/store";
import type { BoardItem, Item, ItemId, ItemOf, LayerId } from "../core/types";
import type { Units } from "../core/units";

const ROTATIONS: [number, string][] = [
  [0, "0°"],
  [90, "90°"],
  [180, "180°"],
  [270, "270°"],
];

/** Board copper layers from the stack (falls back to 2 layers). */
export function copperLayerOptions(doc: DocState): [LayerId, string][] {
  const board = [...doc.items.values()].find((i): i is BoardItem => i.kind === "board");
  const ids = board?.stackup.layers.filter((l) => l.kind === "copper" && l.layer !== null).map((l) => [l.layer as LayerId, l.name] as [LayerId, string]);
  return ids?.length ? ids : [[0, layer(0).name], [1, layer(1).name]];
}

/** Net names of the board, loaded once per document revision. */
function useNets(docId: string): string[] {
  const [nets, setNets] = useState<string[]>([]);
  const items = useEditor((s) => s.docs[docId]?.items);
  useEffect(() => {
    let live = true;
    api
      .pcbNets(docId)
      .then((n) => live && setNets(n))
      .catch(() => {});
    return () => {
      live = false;
    };
    // Nets only change with footprints/ECOs; refetch when the item map is replaced.
  }, [docId, items]);
  return nets;
}

function NetInput({ docId, value, onCommit }: { docId: string; value: string; onCommit: (v: string) => void }) {
  const nets = useNets(docId);
  const id = useId();
  return (
    <>
      <CommitInput mono value={value} placeholder="No net" list={id} onCommit={onCommit} />
      <datalist id={id}>
        {nets.map((n) => (
          <option key={n} value={n} />
        ))}
      </datalist>
    </>
  );
}

function NetRow({ doc, net, onCommit }: { doc: DocState; net: string; onCommit: (v: string) => void }) {
  return (
    <Row label="Net">
      <div className="flex gap-1">
        <NetInput docId={doc.info.id} value={net} onCommit={onCommit} />
        {net && (
          <Button variant="ghost" size="sm" className="h-7 px-1.5 text-[11px]" title="Highlight net" onClick={() => void highlightNetOnBoard(doc.info.id, net)}>
            Show
          </Button>
        )}
      </div>
    </Row>
  );
}

export function BoardItemEditor({ doc, id, item, put, units }: { doc: DocState; id: ItemId; item: Item; put: (i: Item) => void; units: Units }): ReactNode {
  const L_ = (v: number, f: (n: number) => void, min?: number) => <LengthInput value={v} units={units} onCommit={f} min={min} />;
  const copper = copperLayerOptions(doc);
  const len = (a: { x: number; y: number }, b: { x: number; y: number }) => Math.hypot(b.x - a.x, b.y - a.y);
  switch (item.kind) {
    case "footprint":
      return <FootprintEditor doc={doc} fp={item} put={put} units={units} />;
    case "track":
      return (
        <Section title="Track">
          <NetRow doc={doc} net={item.net} onCommit={(net) => put({ ...item, net })} />
          <Row label="Layer">
            <Select value={item.layer} options={copper} onChange={(l) => put({ ...item, layer: l })} />
          </Row>
          <Row label="Width">{L_(item.width, (width) => put({ ...item, width }), 1000)}</Row>
          <Row label="Start X">{L_(item.a.x, (x) => put({ ...item, a: { ...item.a, x } }))}</Row>
          <Row label="Start Y">{L_(item.a.y, (y) => put({ ...item, a: { ...item.a, y } }))}</Row>
          <Row label="End X">{L_(item.b.x, (x) => put({ ...item, b: { ...item.b, x } }))}</Row>
          <Row label="End Y">{L_(item.b.y, (y) => put({ ...item, b: { ...item.b, y } }))}</Row>
          <Row label="Length">
            <span className="font-mono text-ink">{(len(item.a, item.b) / 1e6).toFixed(3)} mm</span>
          </Row>
        </Section>
      );
    case "arcTrack":
      return (
        <Section title="Arc track">
          <NetRow doc={doc} net={item.net} onCommit={(net) => put({ ...item, net })} />
          <Row label="Layer">
            <Select value={item.layer} options={copper} onChange={(l) => put({ ...item, layer: l })} />
          </Row>
          <Row label="Width">{L_(item.width, (width) => put({ ...item, width }), 1000)}</Row>
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
    case "via":
      return (
        <Section title="Via">
          <NetRow doc={doc} net={item.net} onCommit={(net) => put({ ...item, net })} />
          <Row label="X">{L_(item.pos.x, (x) => put({ ...item, pos: { ...item.pos, x } }))}</Row>
          <Row label="Y">{L_(item.pos.y, (y) => put({ ...item, pos: { ...item.pos, y } }))}</Row>
          <Row label="Diameter">{L_(item.diameter, (diameter) => put({ ...item, diameter }), 1000)}</Row>
          <Row label="Drill">{L_(item.drill, (drill) => put({ ...item, drill }), 1000)}</Row>
          <Row label="From layer">
            <Select value={item.fromLayer} options={copper} onChange={(fromLayer) => put({ ...item, fromLayer })} />
          </Row>
          <Row label="To layer">
            <Select value={item.toLayer} options={copper} onChange={(toLayer) => put({ ...item, toLayer })} />
          </Row>
        </Section>
      );
    case "zone":
      return <ZoneEditor doc={doc} id={id} zone={item} put={put} units={units} />;
    default:
      return null;
  }
}

function FootprintEditor({ doc, fp, put, units }: { doc: DocState; fp: ItemOf<"footprint">; put: (i: Item) => void; units: Units }) {
  const L_ = (v: number, f: (n: number) => void) => <LengthInput value={v} units={units} onCommit={f} />;
  const pads = Object.entries(fp.padNets).sort(([a], [b]) => a.localeCompare(b, undefined, { numeric: true }));
  return (
    <>
      <Section title="Component">
        <Row label="Designator">
          <CommitInput mono value={fp.designator} onCommit={(designator) => put({ ...fp, designator })} />
        </Row>
        <Row label="Value">
          <CommitInput value={fp.value} onCommit={(value) => put({ ...fp, value })} />
        </Row>
        <Row label="Footprint">
          <span className="truncate font-mono text-ink" title={fp.footprint}>
            {fp.footprint}
          </span>
        </Row>
        {!doc.footprints[fp.footprint] && (
          <Row label="">
            <span className="text-[11px] text-err-line">Footprint not found in any open library</span>
          </Row>
        )}
        <Row label="Side">
          <Select
            value={fp.side}
            options={[
              ["top", "Top"],
              ["bottom", "Bottom"],
            ]}
            onChange={(side) => put({ ...fp, side })}
          />
        </Row>
        <Row label="X">{L_(fp.pos.x, (x) => put({ ...fp, pos: { ...fp.pos, x } }))}</Row>
        <Row label="Y">{L_(fp.pos.y, (y) => put({ ...fp, pos: { ...fp.pos, y } }))}</Row>
        <Row label="Rotation">
          <NumberInput value={fp.rotation} onCommit={(rotation) => put({ ...fp, rotation: ((rotation % 360) + 360) % 360 })} />
        </Row>
        <Row label="Locked">
          <Check value={fp.locked} onChange={(locked) => put({ ...fp, locked })} />
        </Row>
        <Row label="Not fitted">
          <Check
            value={fp.parameters.DNP === "1"}
            onChange={(v) => {
              const parameters = { ...fp.parameters };
              if (v) parameters.DNP = "1";
              else delete parameters.DNP;
              put({ ...fp, parameters });
            }}
          />
        </Row>
        <Row label="">
          <Button variant="outline" size="sm" className="h-6 text-[11px]" onClick={() => bus.emit("show3d", { docId: doc.info.id })}>
            View in 3D
          </Button>
        </Row>
      </Section>
      {pads.length > 0 && (
        <Section title={`Pads (${pads.length})`}>
          <div className="grid max-h-56 grid-cols-[auto_1fr] gap-x-3 gap-y-0.5 overflow-auto font-mono text-[11px]">
            {pads.map(([pad, net]) => [
              <span key={`${pad}p`} className="text-muted-foreground">
                {pad}
              </span>,
              <button key={`${pad}n`} className="truncate text-left hover:text-accent-ink" onClick={() => net && void highlightNetOnBoard(doc.info.id, net)}>
                {net || "—"}
              </button>,
            ])}
          </div>
        </Section>
      )}
    </>
  );
}

function ZoneEditor({ doc, id, zone, put, units }: { doc: DocState; id: ItemId; zone: ItemOf<"zone">; put: (i: Item) => void; units: Units }) {
  const L_ = (v: number, f: (n: number) => void, min?: number) => <LengthInput value={v} units={units} onCommit={f} min={min} />;
  const copper = copperLayerOptions(doc);
  return (
    <Section title={zone.keepout ? "Keep-out area" : "Polygon pour"}>
      <Row label="Name">
        <CommitInput value={zone.name} onCommit={(name) => put({ ...zone, name })} />
      </Row>
      {!zone.keepout && <NetRow doc={doc} net={zone.net} onCommit={(net) => put({ ...zone, net })} />}
      <Row label="Layer">
        <Select value={zone.layer} options={zone.keepout ? [...copper, [15, "All copper layers"]] : copper} onChange={(l) => put({ ...zone, layer: l })} />
      </Row>
      {!zone.keepout && (
        <>
          <Row label="Priority">
            <NumberInput value={zone.priority} onCommit={(priority) => put({ ...zone, priority: Math.max(0, Math.round(priority)) })} />
          </Row>
          <Row label="Clearance">{L_(zone.clearance, (clearance) => put({ ...zone, clearance }), 0)}</Row>
          <Row label="">
            <span className="text-[11px] leading-snug text-faint">The pour keeps the larger of this and the clearance rules.</span>
          </Row>
          <Row label="Min width">{L_(zone.minWidth, (minWidth) => put({ ...zone, minWidth }), 0)}</Row>
          <Row label="Pad connect">
            <Select
              value={zone.connect}
              options={[
                ["thermal", "Thermal relief"],
                ["solid", "Direct"],
                ["none", "None"],
              ]}
              onChange={(connect) => put({ ...zone, connect })}
            />
          </Row>
          {zone.connect === "thermal" && (
            <>
              <Row label="Thermal gap">{L_(zone.thermalGap, (thermalGap) => put({ ...zone, thermalGap }), 0)}</Row>
              <Row label="Spoke width">{L_(zone.spokeWidth, (spokeWidth) => put({ ...zone, spokeWidth }), 0)}</Row>
            </>
          )}
          <Row label="Remove islands">
            <Check value={zone.removeIslands} onChange={(removeIslands) => put({ ...zone, removeIslands })} />
          </Row>
          <Row label="Fill">
            <span className="font-mono text-ink">{zone.fill.length ? `${zone.fill.length} region${zone.fill.length > 1 ? "s" : ""}` : "unfilled"}</span>
          </Row>
          <Row label="">
            <div className="flex gap-1">
              <Button variant="outline" size="sm" className="h-6 gap-1 text-[11px]" onClick={() => void repour(doc.info.id, id)}>
                <RefreshCw /> Repour
              </Button>
              <Button variant="outline" size="sm" className="h-6 gap-1 text-[11px]" title="E R" onClick={() => useEditor.getState().patchDoc(doc.info.id, { tool: "reshape" })}>
                <Scissors /> Reshape
              </Button>
            </div>
          </Row>
        </>
      )}
    </Section>
  );
}

/** Placement options for board tools; returns null for tools it doesn't know. */
export function boardPlacementRows(impl: string, pl: Placement, set: (p: Partial<Placement>) => void, units: Units, doc: DocState | undefined): ReactNode[] | null {
  const L_ = (v: number, f: (n: number) => void, min?: number) => <LengthInput value={v} units={units} onCommit={f} min={min} />;
  const docId = doc?.info.id ?? "";
  const net = (
    <Row key="net" label="Net">
      <NetInput docId={docId} value={pl.net} onCommit={(n) => set({ net: n })} />
    </Row>
  );
  switch (impl) {
    case "route":
    case "diffPair":
      return [
        <Row key="m" label="Mode">
          <Select
            value={pl.routeMode}
            options={[
              ["walkaround", "Walk around obstacles"],
              ["shove", "Push obstacles"],
              ["ignore", "Ignore obstacles"],
            ]}
            onChange={(routeMode) => set({ routeMode })}
          />
        </Row>,
        <Row key="c" label="Corners">
          <Select
            value={pl.cornerStyle}
            options={[
              ["diagonal45", "45°"],
              ["orthogonal90", "90°"],
              ["any", "Any angle"],
            ]}
            onChange={(cornerStyle) => set({ cornerStyle })}
          />
        </Row>,
        <Row key="w" label="Width">
          <div className="flex items-center gap-1">
            {pl.trackWidth === null ? (
              <span className="flex-1 text-[11px] text-muted-foreground">From rules</span>
            ) : (
              L_(pl.trackWidth, (trackWidth) => set({ trackWidth }), 1000)
            )}
            <Button variant="ghost" size="sm" className="h-7 px-1.5 text-[11px]" onClick={() => set({ trackWidth: pl.trackWidth === null ? 250_000 : null })}>
              {pl.trackWidth === null ? "Override" : "Rules"}
            </Button>
          </div>
        </Row>,
        <Row key="h" label="">
          <span className="text-[11px] leading-snug text-faint">Click a pad or track to start. Space flips the corner, * adds a via and switches layers, Shift+R cycles the mode, Esc finishes.</span>
        </Row>,
      ];
    case "zone":
      return [
        net,
        <Row key="p" label="Priority">
          <NumberInput value={pl.zonePriority} onCommit={(zonePriority) => set({ zonePriority: Math.max(0, Math.round(zonePriority)) })} />
        </Row>,
      ];
    case "via":
      return [net];
    case "keepout":
    case "outline":
      return [];
    case "footprint":
      return pl.footprint
        ? [
            <Row key="f" label="Footprint">
              <span className="truncate font-mono text-ink">{pl.footprint.key}</span>
            </Row>,
            <Row key="d" label="Designator">
              <CommitInput mono value={pl.footprint.designator} onCommit={(designator) => set({ footprint: { ...pl.footprint!, designator } })} />
            </Row>,
            <Row key="r" label="Rotation">
              <Select value={pl.rotation} options={ROTATIONS} onChange={(rotation) => set({ rotation })} />
            </Row>,
            <Row key="s" label="Side">
              <Select
                value={pl.mirror ? 1 : 0}
                options={[
                  [0, "Top"],
                  [1, "Bottom"],
                ]}
                onChange={(v) => set({ mirror: v === 1 })}
              />
            </Row>,
          ]
        : [];
    default:
      return null;
  }
}
