import type { TuneRequest } from '../core/api';
import type { DocState } from '../core/store';
import type { TrackItem } from '../core/types';

export type TuningSource = Pick<DocState, 'items' | 'footprints' | 'epoch'>;
export interface TuningPreviewResult {
  docId: string;
  key: string;
  source: TuningSource;
  tracks: TrackItem[];
  error: string | null;
}
export function tuningPreviewCurrent(plan: TuningPreviewResult | null, docId: string, key: string, source: TuningSource | undefined): boolean {
  return !!plan && !!source && plan.docId === docId && plan.key === key
    && plan.source.items === source.items && plan.source.footprints === source.footprints && plan.source.epoch === source.epoch;
}

/** Debounced, read-only planning, with at most one native job in flight. */
export class TuningPreview {
  private generation = 0;
  private stopTimer?: () => void;
  private active = false;
  private queued?: { token: number; docId: string; req: TuneRequest; source: TuningSource; key: string; deliver: (result: TuningPreviewResult) => void };
  constructor(private load: (docId: string, req: TuneRequest) => Promise<TrackItem[]>,
    private schedule: (run: () => void) => () => void = run => { const timer = setTimeout(run, 200); return () => clearTimeout(timer); }) {}
  cancel() { this.generation++; this.stopTimer?.(); this.stopTimer = undefined; this.queued = undefined; }
  request(docId: string, req: TuneRequest, source: TuningSource, deliver: (result: TuningPreviewResult) => void) {
    this.cancel();
    const token = this.generation;
    const snapshot = { items: source.items, footprints: source.footprints, epoch: source.epoch };
    const key = JSON.stringify(req);
    this.stopTimer = this.schedule(() => {
      this.stopTimer = undefined;
      if (token !== this.generation) return;
      this.queued = { token, docId, req: JSON.parse(key) as TuneRequest, source: snapshot, key, deliver };
      this.pump();
    });
    return () => { if (token === this.generation) this.cancel(); };
  }
  private pump() {
    if (this.active || !this.queued) return;
    const job = this.queued;this.queued = undefined;this.active = true;
    void Promise.resolve().then(() => job.token === this.generation ? this.load(job.docId, job.req) : null).then(tracks => {
      if (tracks && job.token === this.generation) job.deliver({ docId:job.docId,key:job.key,source:job.source,tracks,error:null });
    }).catch(error => {
      if (job.token === this.generation) job.deliver({ docId:job.docId,key:job.key,source:job.source,tracks:[],error:String(error) });
    }).finally(() => { this.active = false;this.pump(); });
  }
}
