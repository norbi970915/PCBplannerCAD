import test from 'node:test';
import assert from 'node:assert/strict';
import { alignFootprints, boardStats, distributeFootprints, unrouteFootprints, walkCopper } from '../src/pcb/boardTools.ts';
import type { Footprint, Item } from '../src/core/types.ts';

const mm = (v: number) => Math.round(v * 1e6);
const p = (x: number, y: number) => ({ x: mm(x), y: mm(y) });
const tr = (a: { x: number; y: number }, b: { x: number; y: number }, net = 'N', layer = 0): Item => ({ kind: 'track', a, b, width: mm(0.25), layer, net });
const via = (at: { x: number; y: number }, net = 'N', drill = 0.3): Item => ({ kind: 'via', pos: at, diameter: mm(0.6), drill: mm(drill), fromLayer: 0, toLayer: 1, net });
const pad = (number: string, x: number, padKind: 'smd' | 'tht' | 'npth' = 'smd', w = 1) =>
  ({ kind: 'pad', number, pos: p(x, 0), width: mm(w), height: mm(w), rotation: 0, shape: 'rect', padKind, layer: padKind === 'smd' ? 0 : 15, drill: padKind === 'smd' ? null : { width: mm(0.8), height: mm(0.8) }, cornerRatio: 0 }) as Item;
// "R": two 1×1 mm SMD pads at x = ±1 mm (box 3 × 1 mm). "J": one THT pad.
const footprints: Record<string, Footprint> = {
  R: { name: 'R', description: '', items: [pad('1', -1), pad('2', 1)], model: null },
  J: { name: 'J', description: '', items: [pad('1', 0, 'tht', 1.6)], model: null },
};
const part = (designator: string, x: number, y: number, nets: Record<string, string> = {}, extra: Partial<Item> = {}): Item =>
  ({ kind: 'footprint', designator, value: '', footprint: 'R', pos: p(x, y), rotation: 0, side: 'top', locked: false, padNets: nets, component: '', parameters: {}, ...extra }) as Item;

test('align moves every unlocked footprint onto the outermost edge or the common centre', () => {
  const items = new Map<number, Item>([[1, part('R1', 0, 0)], [2, part('R2', 10, 4)], [3, part('R3', 5, -3, {}, { locked: true })]]);
  const left = new Map(alignFootprints(items, footprints, [1, 2, 3], 'left').map((r) => [r.id, r.item]));
  assert.deepEqual([...left.keys()], [2], 'R1 already defines the left edge; locked R3 stays');
  assert.deepEqual((left.get(2) as { pos: unknown }).pos, p(0, 4));
  const top = alignFootprints(items, footprints, [1, 2], 'top');
  assert.deepEqual(top.map((r) => [r.id, (r.item as { pos: unknown }).pos]), [[1, p(0, 4)]], 'Y up: top is the larger y');
  const mid = alignFootprints(items, footprints, [1, 2], 'centerY');
  assert.deepEqual(mid.map((r) => (r.item as { pos: { y: number } }).pos.y), [mm(2), mm(2)]);
});

test('distribute leaves equal gaps between footprint extents and keeps the outer two', () => {
  const items = new Map<number, Item>([[1, part('R1', 0, 0)], [2, part('R2', 2, 0)], [3, part('R3', 3, 0)], [4, part('R4', 12, 0)]]);
  const out = distributeFootprints(items, footprints, [1, 2, 3, 4], 'x');
  // Span −1.5…13.5 = 15 mm, four 3 mm boxes → 1 mm gaps: centres at 0, 4, 8, 12.
  assert.deepEqual(out.map((r) => [r.id, (r.item as { pos: { x: number } }).pos.x]), [[2, mm(4)], [3, mm(8)]]);
  assert.deepEqual(distributeFootprints(items, footprints, [1, 2], 'x'), [], 'needs three');
});

test('board statistics count parts, pads, via kinds, track length and build the drill table', () => {
  const items = new Map<number, Item>([
    [1, part('R1', 0, 0, { '1': 'A' })],
    [2, part('J1', 10, 0, { '1': 'B' }, { footprint: 'J', side: 'bottom' } as Partial<Item>)],
    [3, tr(p(0, 0), p(3, 4))],
    [4, via(p(5, 5))], [5, via(p(6, 5))], [6, via(p(7, 5), 'N', 0.2)],
    [7, { kind: 'line', a: p(-5, -5), b: p(25, -5), width: mm(0.1), layer: 14 } as Item],
    [8, { kind: 'line', a: p(25, -5), b: p(25, 15), width: mm(0.1), layer: 14 } as Item],
  ]);
  const s = boardStats(items, footprints, [{ net: 'A', a: p(0, 0), b: p(1, 1) }]);
  assert.deepEqual(s.footprints, { total: 2, top: 1, bottom: 1, smd: 1, tht: 1, mixed: 0, locked: 0 });
  assert.deepEqual(s.pads, { smd: 2, tht: 1, npth: 0 });
  assert.equal(s.vias.through, 3);
  assert.equal(s.tracks.length, mm(5));
  assert.equal(s.nets, 2);
  assert.equal(s.unrouted, 1);
  assert.deepEqual(s.size, { width: mm(30), height: mm(20) });
  assert.deepEqual(s.drills.map((d) => [d.label, d.kind, d.width, d.count]), [['T1', 'via', mm(0.2), 1], ['T2', 'via', mm(0.3), 2], ['T3', 'pad', mm(0.8), 1]]);
});

// R1 pad 2 at (1,0) → (4,0) → junction (4,0) branches to (4,3) and (8,0) → via at (8,0) → bottom to J1 at (12,0).
const routed = () => new Map<number, Item>([
  [1, part('R1', 0, 0, { '2': 'N' })],
  [2, part('J1', 12, 0, { '1': 'N' }, { footprint: 'J' } as Partial<Item>)],
  [10, tr(p(1, 0), p(2, 0))], [11, tr(p(2, 0), p(4, 0))],
  [12, tr(p(4, 0), p(4, 3))], [13, tr(p(4, 0), p(8, 0))],
  [14, via(p(8, 0))], [15, tr(p(8, 0), p(12, 0), 'N', 1)],
]);

test('U selects the segment chain, a second U the whole connection up to the pads', () => {
  const items = routed();
  assert.deepEqual([...walkCopper(items, footprints, [10], 'chain')].sort(), [10, 11], 'stops at the pad and the junction');
  assert.deepEqual([...walkCopper(items, footprints, [13], 'chain')].sort(), [13], 'stops at the via');
  assert.deepEqual([...walkCopper(items, footprints, [10], 'connection')].sort((a, b) => a - b), [10, 11, 12, 13, 14, 15]);
});

test('unroute removes the routing of the selected part up to the first junction', () => {
  assert.deepEqual(unrouteFootprints(routed(), footprints, [1]).sort(), [10, 11]);
  // From J1: the bottom track and the via go; the top routing beyond the via stays (it joins the junction).
  assert.deepEqual(unrouteFootprints(routed(), footprints, [2]).sort((a, b) => a - b), [13, 14, 15]);
  const poured = routed();
  poured.set(20, { kind: 'zone', outline: [p(6, -2), p(10, -2), p(10, 2), p(6, 2)], layer: 0, net: 'N', name: '', priority: 0, clearance: 0, minWidth: 0, connect: 'solid', thermalGap: 0, spokeWidth: 0, keepout: false, removeIslands: true, fill: [] } as Item);
  assert.deepEqual(unrouteFootprints(poured, footprints, [2]), [15], 'a via in its own pour stays');
});
