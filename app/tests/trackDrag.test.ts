import test from 'node:test';
import assert from 'node:assert/strict';
import { applyTrackDrag, planTrackDrag, trackDragIds } from '../src/editor/trackDrag.ts';
import type { Item } from '../src/core/types.ts';

const mm = (v: number) => Math.round(v * 1e6);
const p = (x: number, y: number) => ({ x: mm(x), y: mm(y) });
const tr = (a: { x: number; y: number }, b: { x: number; y: number }, net = 'N'): Item => ({ kind: 'track', a, b, width: mm(0.25), layer: 0, net });

// 45° in, horizontal middle, 45° out: (0,0)→(2,2)→(6,2)→(8,0).
const board = () => new Map<number, Item>([[1, tr(p(0, 0), p(2, 2))], [2, tr(p(2, 2), p(6, 2))], [3, tr(p(6, 2), p(8, 0))]]);

test('dragging a middle segment keeps the 45° neighbours connected and at 45°', () => {
  const items = board(); const plan = planTrackDrag(items, {}, [2])!;
  assert.deepEqual(trackDragIds(plan).sort(), [1, 2, 3]);
  const { replace, add } = applyTrackDrag(plan, p(0.3, 1));
  const out = new Map(replace.map((r) => [r.id, r.item]));
  assert.deepEqual(add, []);
  assert.deepEqual(out.get(2)!.a, p(3, 3)); assert.deepEqual(out.get(2)!.b, p(5, 3), 'moved straight up, ends slid along the diagonals');
  assert.deepEqual(out.get(1)!.b, p(3, 3)); assert.deepEqual(out.get(1)!.a, p(0, 0), 'far ends stay put');
  assert.deepEqual(out.get(3)!.a, p(5, 3)); assert.deepEqual(out.get(3)!.b, p(8, 0));
});

test('sliding a single segment along itself changes nothing', () => {
  const plan = planTrackDrag(board(), {}, [2])!;
  const out = new Map(applyTrackDrag(plan, p(1.5, 0)).replace.map((r) => [r.id, r.item]));
  assert.deepEqual(out.get(2)!.a, p(2, 2)); assert.deepEqual(out.get(2)!.b, p(6, 2));
});

test('an end held by a via gets a joining segment instead of leaving a gap', () => {
  const items = board(); items.delete(3);
  items.set(9, { kind: 'via', pos: p(6, 2), diameter: mm(0.6), drill: mm(0.3), fromLayer: 0, toLayer: 1, net: 'N' });
  const { replace, add } = applyTrackDrag(planTrackDrag(items, {}, [2])!, p(0, 1));
  const moved = replace.find((r) => r.id === 2)!.item;
  assert.deepEqual(moved.b, p(6, 3));
  assert.deepEqual(add.map((t) => [t.a, t.b]), [[p(6, 2), p(6, 3)]], 'the via stays and a stub reconnects it');
});

test('other nets and other layers are never dragged along', () => {
  const items = board(); items.set(4, tr(p(6, 2), p(6, 9), 'OTHER')); items.set(5, { ...tr(p(6, 2), p(9, 9)), layer: 1 } as Item);
  assert(!trackDragIds(planTrackDrag(items, {}, [2])!).some((id) => id === 4 || id === 5));
});

test('selections with non-track items fall back to an ordinary move', () => {
  const items = board(); items.set(7, { kind: 'via', pos: p(1, 1), diameter: 1, drill: 1, fromLayer: 0, toLayer: 1, net: 'N' });
  assert.equal(planTrackDrag(items, {}, [2, 7]), null);
});

test('a T-junction stretches every connected branch to the new point', () => {
  const items = board(); items.set(4, tr(p(6, 2), p(6, 6)));
  const out = new Map(applyTrackDrag(planTrackDrag(items, {}, [2])!, p(0, 1)).replace.map((r) => [r.id, r.item]));
  assert.deepEqual(out.get(2)!.b, p(6, 3)); assert.deepEqual(out.get(3)!.a, p(6, 3)); assert.deepEqual(out.get(4)!.a, p(6, 3));
});

test('moving a part previews its pad segment sliding at the same angle along its neighbour', async () => {
  const { attachedTracks, stretchAttached, attachedIds } = await import('../src/editor/trackDrag.ts');
  const pad = { kind: 'pad', number: '1', pos: p(0, 0), width: mm(1), height: mm(1), rotation: 0, shape: 'rect', padKind: 'smd', layer: 0, drill: null, cornerRatio: 0 };
  const items = new Map<number, Item>([
    [1, { kind: 'footprint', pos: p(20, 0), rotation: 0, side: 'top', footprint: 'F', padNets: { '1': 'N' }, designator: 'U2', value: '', component: '', locked: false, parameters: {} } as Item],
    [2, tr(p(15, 5), p(20, 0))], [3, tr(p(5, 5), p(15, 5))],
  ]);
  const att = attachedTracks(items, { F: { name: 'F', description: '', items: [pad], model: null } } as never, [1]);
  assert.deepEqual(attachedIds(att).sort(), [2, 3]);
  const [seg, nb] = stretchAttached(att, p(0, -3), true);
  assert.deepEqual([seg.a, seg.b], [p(12, 5), p(20, -3)], '45° kept, far end slid to (12,5)');
  assert.deepEqual([nb.a, nb.b], [p(5, 5), p(12, 5)], 'the horizontal neighbour got shorter');
  const [plain] = stretchAttached(att, p(0, -3), false);
  assert.deepEqual([plain.a, plain.b], [p(15, 5), p(20, -3)], 'Free mode stretches straight');
});
