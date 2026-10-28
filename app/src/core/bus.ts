/** Typed pub/sub for UI-level requests (open a panel, focus a view…). */
export interface BusEvents {
  /** Show a document in a dock tab (schematic, pcb, footprint). */
  showDoc: { docId: string };
  /** Show the symbol editor for a library symbol. */
  showSymbol: { libId: string; name: string; unit?: number };
  /** Show a library browser tab. */
  showLibrary: { libId: string; select?: { kind: "symbol" | "footprint" | "component"; name: string } };
  /** Close dock tabs belonging to documents that no longer exist. */
  docsClosed: { docIds: string[] };
  libraryClosed: { libId: string };
  /** Canvas view commands for the active document. */
  view: { action: "fit" | "zoomIn" | "zoomOut" | "fitSelection" };
  /** Attach items of a document to its canvas cursor as a floating move. */
  pickUp: { docId: string; ids: number[] };
  canvasTimings: { docId: string };
  resetLayout: Record<string, never>;
  showPanel: { id: "projects" | "components" | "properties" | "messages" | "scripts" | "pcb" };
  /** Open the 3D viewer for a board. */
  show3d: { docId: string };
  /** Open the Gerber viewer (optionally with a folder/zip to load). */
  showGerbers: { path?: string };
}

type Handler<T> = (payload: T) => void;

class Bus {
  private handlers = new Map<keyof BusEvents, Set<Handler<never>>>();

  on<K extends keyof BusEvents>(event: K, fn: Handler<BusEvents[K]>): () => void {
    let set = this.handlers.get(event);
    if (!set) this.handlers.set(event, (set = new Set()));
    set.add(fn as Handler<never>);
    return () => set.delete(fn as Handler<never>);
  }

  emit<K extends keyof BusEvents>(event: K, payload: BusEvents[K]) {
    this.handlers.get(event)?.forEach((fn) => (fn as Handler<BusEvents[K]>)(payload));
  }
}

export const bus = new Bus();
