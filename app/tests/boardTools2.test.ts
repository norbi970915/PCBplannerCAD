import test from 'node:test';
import assert from 'node:assert/strict';
import { netRows, packFootprints } from '../src/pcb/boardTools.ts';
import { findMatches, matcher } from '../src/core/find.ts';
import { dimensionLabel, dimensionPrims, dimensionThrough } from '../src/editor/dimension.ts';
import type { DimensionItem, Footprint, Item } from '../src/core/types.ts';

const mm = (v: number) => Math.round(v * 1e6);
const p = (x: number, y: number) => ({ x: mm(x), y: mm(y) });
const pad = (number: string, x: number) => ({ kind: 'pad', number, pos: p(x, 0), width: mm(1), height: mm(1), rotation: 0, shape: 'rect', padKind: 'smd', layer: 0, drill: null, cornerRatio: 0 }) as Item;
const footprints: Record<string, Footprint> = {
  R: { name: 'R', description: '', items: [pad('1', -1), pad('2', 1)], model: null }, // 3 × 1 mm
  U: { name: 'U', description: '', items: [pad('1', -4), pad('2', 4), { ...pad('3', 0), pos: p(0, 3) } as Item], model: null }, // 9 × 4 mm
};
const part = (designator: string, footprint: string, x: number, y: number, value = '', nets: Record<string, string> = {}): Item =>
  ({ kind: 'footprint', designator, value, footprint, pos: p(x, y), rotation: 0, side: 'top', locked: false, padNets: nets, component: '', parameters: {} }) as Item;

test('find ranks exact designators first and supports wildcards and nets', () => {
  const items = new Map<number, Item>([
    [1, part('R1', 'R', 0, 0, '10k', { '1': 'GND' })],
    [2, part('R12', 'R', 5, 0, '1k')],
    [3, part('U1', 'U', 20, 0, 'R1-buffer')],
    [4, { kind: 'track', a: p(0, 0), b: p(1, 0), width: mm(0.2), layer: 0, net: 'GND' } as Item],
  ]);
  const r1 = findMatches(items, 'r1');
  assert.deepEqual(r1.map((m) => m.label), ['R1', 'R12', 'U1'], 'exact, then prefix, then a value match');
  assert.deepEqual(findMatches(items, 'R?').map((m) => m.label), ['R1']);
  const gnd = findMatches(items, 'gnd');
  assert.equal(gnd[0].kind, 'net');
  assert.deepEqual(gnd[0].ids, [1, 4]);
  assert.equal(matcher('  '), null);
  assert.deepEqual(findMatches(items, '10K').map((m) => m.label), ['R1'], 'values match case-insensitively');
});

test('pack & move puts footprints in tight rows from the top-left, tallest first, on whole grid steps', () => {
  const items = new Map<number, Item>([[1, part('R1', 'R', 50, 0)], [2, part('U1', 'U', 0, 20)], [3, part('R2', 'R', 30, -10)]]);
  const moved = new Map(packFootprints(items, footprints, [1, 2, 3], mm(1), mm(0.5)).map((r) => [r.id, (r.item as { pos: { x: number; y: number } }).pos]));
  // Selection box: left −4.5 (U1), top 23.5 (U1's top pad). Row width ≈ 9.75 mm (√ of the gapped areas × 1.2):
  // U1 (tallest) fills the first row, R1 and R2 share the second, 1 mm below it.
  assert.equal(moved.get(2), undefined, 'U1 already sits at the top-left');
  assert.deepEqual(moved.get(1), p(-3, 18));
  assert.deepEqual(moved.get(3), p(1, 18), 'R2 right of R1 with a 1 mm gap');
});

test('nets panel rows split routed length per layer', () => {
  const items = new Map<number, Item>([
    [1, { kind: 'track', a: p(0, 0), b: p(3, 4), width: mm(0.2), layer: 0, net: 'A' } as Item],
    [2, { kind: 'track', a: p(0, 0), b: p(0, 2), width: mm(0.2), layer: 1, net: 'A' } as Item],
    [3, { kind: 'via', pos: p(0, 0), diameter: mm(0.6), drill: mm(0.3), fromLayer: 0, toLayer: 1, net: 'A' } as Item],
  ]);
  const [row] = netRows(items, []).rows;
  assert.equal(row.length, mm(7));
  assert.deepEqual([...row.byLayer], [[0, mm(5)], [1, mm(2)]]);
  assert.equal(row.vias, 1);
});

const base = { kind: 'dimension', layer: 4, width: mm(0.1), textHeight: mm(1), units: 'mm', precision: 2 } as const;

test('dimension geometry matches the core: line at the offset, value beside it, readable text', () => {
  const d: DimensionItem = { ...base, a: p(0, 0), b: p(20, 3), offset: mm(5), orthogonal: true, angle: 0 };
  const prims = dimensionPrims(d);
  assert.equal(prims.length, 8);
  const line = prims[2] as { a: unknown; b: unknown };
  assert.deepEqual([line.a, line.b], [p(0, 5), p(20, 5)]);
  const text = prims[7] as { text: string; pos: unknown; rotation: number };
  assert.deepEqual([text.text, text.pos, text.rotation], ['20.00 mm', p(10, 5.8), 0]);
  assert.equal(dimensionLabel({ ...d, orthogonal: false, b: p(30, 40) }), '50.00 mm');
  assert.equal(dimensionLabel({ ...d, orthogonal: true, angle: 90, b: p(30, 40), units: 'mil', precision: 0 }), '1575 mil');
});

test('the third click sets the dimension line through the cursor for every style', () => {
  const v = dimensionThrough(p(0, 0), p(10, 4), p(3, -6), 'horizontal', base);
  assert.equal(v.offset, mm(-6));
  assert.equal(dimensionLabel(v), '10.00 mm');
  const vert = dimensionThrough(p(0, 0), p(10, 4), p(14, 1), 'vertical', base);
  assert.equal(vert.offset, mm(-14), 'left normal of +Y is −X');
  assert.equal(dimensionLabel(vert), '4.00 mm');
  const al = dimensionThrough(p(0, 0), p(10, 0), p(5, 2), 'aligned', base);
  assert.equal(al.offset, mm(2));
});
