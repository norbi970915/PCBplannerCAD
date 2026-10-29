import type { EcoChange } from '../core/types';

/** Geometry removal/replacement and connectivity changes require explicit review. */
export function ecoNeedsReview(change: EcoChange): boolean {
  return change.kind === 'removeFootprint' || change.kind === 'changeFootprint'
    || change.kind === 'updateNets' || change.kind === 'renameNet';
}
export function defaultEcoSelection(changes: readonly EcoChange[]): Set<number> {
  return new Set(changes.flatMap((change, index) => change.kind !== 'warning' && !ecoNeedsReview(change) ? [index] : []));
}
