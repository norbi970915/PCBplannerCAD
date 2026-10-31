import type { CleanupOptions, CleanupResult } from "../core/types";
interface SourceDocument { items: unknown; footprints: unknown; epoch?: number }
export interface CleanupSource extends SourceDocument { docId: string; key: string }
export interface CleanupReview { source: CleanupSource; plan: CleanupResult }
const key=(opts:CleanupOptions)=>JSON.stringify([opts.zeroLength,opts.duplicates,opts.dangling,opts.unconnectedVias,opts.mergeCollinear]);
export function cleanupSource(docId:string,opts:CleanupOptions,doc:SourceDocument|undefined):CleanupSource|null {
  return doc?{docId,key:key(opts),items:doc.items,footprints:doc.footprints,epoch:doc.epoch}:null;
}
export function cleanupReviewCurrent(review:CleanupReview|null,docId:string,opts:CleanupOptions,doc:SourceDocument|undefined):boolean {
  return !!review&&!!doc&&review.source.docId===docId&&review.source.key===key(opts)
    &&review.source.items===doc.items&&review.source.footprints===doc.footprints&&review.source.epoch===doc.epoch;
}
