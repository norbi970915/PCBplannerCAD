import type { ZonePolygon } from "../core/types";
import { triangulateWithHoles } from "./shapes";

export type Triangles = readonly number[] | Float64Array;
export interface FillRequest { id: number; polygon: ZonePolygon }
export interface FillResponse { id: number; triangles?: ArrayBuffer; error?: string }
export interface FillWorker {
  onmessage: ((event: { data: FillResponse }) => void) | null;
  onerror: ((event: { preventDefault?: () => void }) => void) | null;
  postMessage(message: FillRequest): void;
  terminate(): void;
}
interface Options {
  worker?: () => FillWorker;
  schedule?: (callback: () => void) => number;
  cancel?: (id: number) => void;
  onBusy?: (busy: boolean) => void;
  onFallback?: () => void;
}

/** Large immutable pours are prepared off the UI thread; only one job is in flight. */
export class PourTriangles {
  private ready = new WeakMap<ZonePolygon, Triangles>();
  private pending = new WeakMap<ZonePolygon, number>();
  private jobs = new Map<number, ZonePolygon>();
  private queue: number[] = [];
  private allowed?: ReadonlySet<ZonePolygon>;
  private worker: FillWorker | null = null;
  private active: number | null = null;
  private nextId = 1;
  private notification: number | null = null;
  private fallback = false;
  private disposed = false;
  private busy = false;
  private schedule: (callback: () => void) => number;
  private cancel: (id: number) => void;

  constructor(private onReady: () => void, private options: Options = {}) {
    this.schedule = options.schedule ?? (callback => requestAnimationFrame(callback));
    this.cancel = options.cancel ?? (id => cancelAnimationFrame(id));
  }

  get pendingCount() { return this.jobs.size; }

  private large(polygon: ZonePolygon) {
    return polygon.holes.length >= 64 || polygon.outer.length + polygon.holes.reduce((n, ring) => n + ring.length, 0) >= 1000;
  }

  /** Preview reads never enqueue transformed polygons on each pointer movement. */
  peek = (polygon: ZonePolygon): Triangles | null => {
    if (this.disposed) return null;
    const triangles = this.ready.get(polygon);
    if (triangles) return triangles;
    if (this.large(polygon)) return null;
    return this.direct(polygon);
  };

  read = (polygon: ZonePolygon): Triangles | null => {
    const triangles = this.ready.get(polygon);
    if (triangles) return triangles;
    if (this.disposed || (this.allowed && !this.allowed.has(polygon))) return null;
    if (this.fallback || !this.large(polygon)) return this.direct(polygon);
    if (!this.pending.has(polygon)) {
      const id = this.nextId++;
      this.pending.set(polygon, id); this.jobs.set(id, polygon); this.queue.push(id);
      this.setBusy(); this.pump();
    }
    return null;
  };

  private direct(polygon: ZonePolygon): Triangles {
    const triangles = triangulateWithHoles(polygon.outer, polygon.holes);
    this.ready.set(polygon, triangles); return triangles;
  }

  /** Cancel work for replaced fills, hidden layers and inactive documents. */
  retain(polygons: ReadonlySet<ZonePolygon>) {
    if (this.disposed) return;
    this.allowed = polygons;
    for (const [id, polygon] of this.jobs) if (!polygons.has(polygon)) {
      this.pending.delete(polygon); this.jobs.delete(id);
    }
    this.queue = this.queue.filter(id => this.jobs.has(id));
    if (this.active !== null && !this.jobs.has(this.active)) {
      this.worker?.terminate(); this.worker = null; this.active = null;
    }
    this.setBusy(); this.pump();
  }

  private setBusy() {
    const busy = this.jobs.size > 0;
    if (busy !== this.busy) { this.busy = busy; this.options.onBusy?.(busy); }
  }

  private notify() {
    if (this.disposed || this.notification !== null) return;
    this.notification = this.schedule(() => {
      this.notification = null;
      if (!this.disposed) this.onReady();
    });
  }

  private fail() {
    if (this.disposed || this.fallback) return;
    this.fallback = true; this.worker?.terminate(); this.worker = null; this.active = null;
    this.jobs.clear(); this.queue = []; this.pending = new WeakMap();
    this.setBusy(); this.options.onFallback?.(); this.notify();
  }

  private pump() {
    if (this.disposed || this.fallback || this.active !== null || !this.queue.length) return;
    try {
      if (!this.worker) {
        const worker = this.options.worker?.() ?? new Worker(new URL("./pourTriangles.worker.ts", import.meta.url), { type: "module" }) as unknown as FillWorker;
        this.worker = worker;
        worker.onmessage = event => {
          if (this.disposed || worker !== this.worker || event.data.id !== this.active) return;
          const { id, triangles, error } = event.data;
          if (error || !(triangles instanceof ArrayBuffer) || triangles.byteLength % 48 !== 0) { this.fail(); return; }
          const polygon = this.jobs.get(id);
          if (polygon) { this.ready.set(polygon, new Float64Array(triangles)); this.pending.delete(polygon); }
          this.jobs.delete(id); this.active = null; this.setBusy(); this.notify(); this.pump();
        };
        worker.onerror = event => { event.preventDefault?.(); if (worker === this.worker) this.fail(); };
      }
      const id = this.queue.shift()!;
      const polygon = this.jobs.get(id);
      if (!polygon) { this.pump(); return; }
      this.active = id; this.worker.postMessage({ id, polygon });
    } catch { this.fail(); }
  }

  dispose() {
    this.disposed = true; this.worker?.terminate(); this.worker = null;
    if (this.notification !== null) this.cancel(this.notification);
    this.notification = null; this.jobs.clear(); this.queue = []; this.pending = new WeakMap();
    this.ready = new WeakMap(); this.allowed = undefined; this.active = null;
  }
}
