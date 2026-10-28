/** Coalesces input and invalidations into one update/draw per animation frame. */
export class InteractionFrame<T> {
  private pending: T | null = null;
  private frame: number | null = null;
  private running = false;
  private disposed = false;
  private observer: ((cpuMs: number) => void) | null = null;
  setFrameObserver(observer: ((cpuMs: number) => void) | null) { this.observer = observer; }

  constructor(
    private update: (value: T) => void,
    private draw: () => void,
    private request: (callback: FrameRequestCallback) => number = callback => globalThis.requestAnimationFrame(callback),
    private cancel: (id: number) => void = id => globalThis.cancelAnimationFrame(id),
  ) {}

  queue(value: T) { this.pending = value; this.invalidate(); }

  /** Preserve event ordering before down/up/key events, even between frames. */
  flush() {
    if (this.disposed || this.pending === null) return;
    const value = this.pending;
    this.pending = null;
    this.update(value);
  }

  invalidate() {
    if (this.disposed || this.running || this.frame !== null) return;
    this.frame = this.request(() => {
      this.frame = null;
      this.running = true;
      const observer = this.observer;
      const began = observer ? performance.now() : 0;
      try { this.flush(); this.draw(); }
      finally { this.running = false; observer?.(performance.now() - began); if (this.pending !== null) this.invalidate(); }
    });
  }

  dispose() {
    this.disposed = true;
    if (this.frame !== null) this.cancel(this.frame);
    this.frame = null;
    this.pending = null;
  }
}
