import test from 'node:test';
import assert from 'node:assert/strict';
import type { EcoChange } from '../src/core/types.ts';
import { defaultEcoSelection, ecoNeedsReview } from '../src/pcb/ecoReview.ts';

test('schematic updates leave removals, replacements and all net changes for explicit review', () => {
  const changes = [
    { kind: 'updateValue', item: 1, designator: 'R1', from: '1k', to: '2k' },
    { kind: 'warning', message: 'Review hierarchy' },
    { kind: 'removeFootprint', item: 2, designator: 'R2' },
    { kind: 'changeFootprint', item: 3, designator: 'U1', from: 'A', to: 'B' },
    { kind: 'updateNets', item: 4, designator: 'J1', changes: [['1', 'OLD', 'NEW']] },
    { kind: 'renameNet', from: 'OLD', to: 'NEW' },
    { kind: 'addFootprint', designator: 'R3', value: '', footprintKey: 'L/R', component: '', padNets: {} },
  ] as EcoChange[];
  assert.deepEqual([...defaultEcoSelection(changes)], [0, 6]);
  assert.deepEqual(changes.map(ecoNeedsReview), [false, false, true, true, true, true, false]);
  assert.equal(changes[4].kind, 'updateNets', 'reviewed actions remain available unchanged');
});
test('default update selection is fresh and cannot leak prior explicit choices', () => {
  const changes = [{kind:'removeFootprint', item:1, designator:'R1'}] as EcoChange[];
  const first = defaultEcoSelection(changes);first.add(0);
  assert.equal(defaultEcoSelection(changes).size, 0);
  assert.equal(defaultEcoSelection([]).size, 0);
});
