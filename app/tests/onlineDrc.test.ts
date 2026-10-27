import test from 'node:test';
import assert from 'node:assert/strict';
import { mergeViolations, touchedBetween } from '../src/pcb/drcMerge.ts';
import { routeModeOf, violatorModeOf } from '../src/core/store.ts';
import type { DrcViolation, Item } from '../src/core/types.ts';

const tr = (x: number, net = 'A'): Item => ({ kind: 'track', a: { x, y: 0 }, b: { x: x + 10, y: 0 }, width: 200, layer: 0, net });
const zone = (fill: number): Item => ({ kind: 'zone', outline: [{ x: 0, y: 0 }, { x: 9, y: 0 }, { x: 9, y: 9 }], layer: 0, net: 'GND', name: '', priority: 0, clearance: 0, minWidth: 0, connect: 'thermal', thermalGap: 0, spokeWidth: 0, keepout: false, removeIslands: true, fill: [{ outer: [{ x: fill, y: 0 }, { x: 9, y: 0 }, { x: 9, y: 9 }], holes: [] }] });
const v = (code: string, items: number[], severity: 'error' | 'warning' = 'error'): DrcViolation => ({ severity, code, rule: '', message: code, items, pos: { x: 0, y: 0 }, layer: 0 });

test('a change re-checks moved and added objects, drops deleted ones, and skips pure refills', () => {
  const a = tr(0), b = tr(50);
  const prev = new Map<number, Item>([[1, a], [2, b], [3, zone(1)], [4, tr(90)]]);
  const next = new Map<number, Item>([[1, a], [2, tr(60)], [3, zone(2)], [5, tr(120)]]);
  const t = touchedBetween(prev, next);
  assert.deepEqual([...t.check].sort(), [2, 5], 'moved track 2 and new track 5; the refilled pour 3 is left alone');
  assert.deepEqual([...t.drop].sort(), [2, 4, 5], 'deleted track 4 loses its markers');
  const moved = new Map(next).set(3, { ...(zone(2) as object), outline: [{ x: 0, y: 0 }, { x: 8, y: 0 }, { x: 8, y: 8 }] } as Item);
  assert.ok(touchedBetween(next, moved).check.has(3), 'a reshaped pour is re-checked');
});

test('markers about changed objects are replaced by the fresh near-DRC, others stay', () => {
  const current = [v('clearance', [2, 7]), v('short', [8, 9]), v('clearance', [4, 6])];
  const fresh = [v('clearance', [2, 7]), v('clearance', [5, 6]), v('unrouted', [5], 'warning')];
  const out = mergeViolations(current, new Set([2, 4, 5]), fresh);
  assert.deepEqual(out.map((x) => `${x.code}:${x.items}`), ['short:8,9', 'clearance:2,7', 'clearance:5,6'], 'deleted track 4 cleared, unrouted left to the ratsnest');
  assert.equal(mergeViolations(out, new Set([2]), [v('clearance', [7, 2])]).length, 3, 'no duplicates when the pair comes back reversed');
});

test('design rules off turns every routing and move mode into ignore', () => {
  const placement = { routeMode: 'shove', moveViolators: 'walkaround' } as never;
  assert.equal(routeModeOf({ drcEnforce: true, placement }), 'shove');
  assert.equal(violatorModeOf({ drcEnforce: true, placement }), 'walkaround');
  assert.equal(routeModeOf({ drcEnforce: false, placement }), 'ignore');
  assert.equal(violatorModeOf({ drcEnforce: false, placement }), 'ignore');
});
