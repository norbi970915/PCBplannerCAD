import test from 'node:test';
import assert from 'node:assert/strict';
import { TuningPreview, tuningPreviewCurrent, type TuningPreviewResult, type TuningSource } from '../src/pcb/tuningPreview.ts';
import type { TuneRequest } from '../src/core/api.ts';
import type { TrackItem } from '../src/core/types.ts';
const req = (targetLength: number): TuneRequest => ({ tracks: [1], targetLength, amplitude: 1e6, spacing: 6e5, style: 'accordion' });
const source = (): TuningSource => ({ items: new Map(), footprints: {}, epoch: 1 });
const tick = () => new Promise(resolve => setImmediate(resolve));
function harness() {
  const timers: { run: () => void; cancelled: boolean }[] = [];
  const calls: { req: TuneRequest; resolve: (tracks: TrackItem[]) => void; reject: (error: Error) => void }[] = [];
  const previews: TuningPreviewResult[] = [];
  const controller = new TuningPreview((_id, request) => new Promise((resolve, reject) => calls.push({ req: request, resolve, reject })), run => {
    const timer = { run, cancelled: false };timers.push(timer);return () => { timer.cancelled = true; };
  });
  const flush = async () => { for (const timer of timers.splice(0)) if (!timer.cancelled) timer.run();await tick(); };
  return { controller, calls, previews, flush };
}
test('tuning preview debounce runs only the latest request and preserves its reviewed source', async () => {
  const h = harness(), doc = source();
  h.controller.request('board', req(20e6), doc, result => h.previews.push(result));
  h.controller.request('board', req(21e6), doc, result => h.previews.push(result));
  await h.flush();assert.equal(h.calls.length, 1);assert.equal(h.calls[0].req.targetLength, 21e6);
  h.calls[0].resolve([]);await tick();
  assert(tuningPreviewCurrent(h.previews[0], 'board', JSON.stringify(req(21e6)), doc));
});
test('slow tuning jobs are bounded and superseded native replies cannot replace a newer preview', async () => {
  const h = harness(), doc = source();
  for (const length of [20e6, 21e6, 22e6]) { h.controller.request('board', req(length), doc, result => h.previews.push(result));await h.flush(); }
  assert.equal(h.calls.length, 1);
  h.calls[0].resolve([]);await tick();assert.equal(h.previews.length, 0);assert.equal(h.calls.length, 2);assert.equal(h.calls[1].req.targetLength, 22e6);
  h.calls[1].resolve([]);await tick();assert.equal(h.previews.length, 1);assert.equal(h.previews[0].key, JSON.stringify(req(22e6)));
});
test('cancelling a tuning dialog clears queued jobs and ignores late results and failures', async () => {
  const h = harness(), doc = source();
  const cancel = h.controller.request('board', req(20e6), doc, result => h.previews.push(result));await h.flush();
  h.controller.request('board', req(21e6), doc, result => h.previews.push(result));await h.flush();
  cancel(); // An old cleanup must not cancel the new request.
  h.controller.cancel();h.calls[0].reject(new Error('late failure'));await tick();
  assert.equal(h.calls.length, 1);assert.equal(h.previews.length, 0);
});
test('reviewed tuning plans expire immediately on geometry, footprint, epoch, target or document changes', async () => {
  const h = harness(), doc = source(), key = JSON.stringify(req(20e6));
  h.controller.request('board', req(20e6), doc, result => h.previews.push(result));await h.flush();h.calls[0].resolve([]);await tick();
  const plan = h.previews[0];assert(tuningPreviewCurrent(plan, 'board', key, doc));
  for (const changed of [{ ...doc, items: new Map(doc.items) }, { ...doc, footprints: {} }, { ...doc, epoch: 2 }]) assert(!tuningPreviewCurrent(plan, 'board', key, changed));
  assert(!tuningPreviewCurrent(plan, 'other', key, doc));assert(!tuningPreviewCurrent(plan, 'board', JSON.stringify(req(21e6)), doc));
});
test('current preview errors are visible and a new request can recover', async () => {
  const h = harness(), doc = source();h.controller.request('board', req(20e6), doc, result => h.previews.push(result));await h.flush();
  h.calls[0].reject(new Error('not enough room'));await tick();assert.match(h.previews[0].error!, /not enough room/);assert.deepEqual(h.previews[0].tracks, []);
  h.controller.request('board', req(21e6), doc, result => h.previews.push(result));await h.flush();h.calls[1].resolve([]);await tick();assert.equal(h.previews[1].error, null);
});
