import type { ReactNode } from 'react';
import { Check } from '@/components/app/fields';
import { Button } from '@/components/ui/button';
import { boardCopper, layer } from '../core/layers';
import { useEditor, type DocState } from '../core/store';
import type { LayerId } from '../core/types';
import { FILTER_KINDS, filterActive, useSelectFilter } from '../core/selectFilter';

/** Copper layers of the stack, then every other layer that holds board items. */
export function filterLayers(doc: DocState): { id: LayerId; name: string }[] {
  const out = boardCopper(doc.items.values());
  const seen = new Set(out.map(x => x.id));
  const rest: LayerId[] = [];
  for (const item of doc.items.values()) {
    if (item.kind === 'footprint' || item.kind === 'via' || !('layer' in item) || typeof item.layer !== 'number') continue;
    if (!seen.has(item.layer)) { seen.add(item.layer); rest.push(item.layer); }
  }
  return [...out, ...rest.sort((a, b) => a - b).map(id => ({ id, name: layer(id).name }))];
}

function Row({ label, value, onChange, onOnly, swatch }: { label: string; value: boolean; onChange: (v: boolean) => void; onOnly: () => void; swatch?: string }) {
  return <div className="flex items-center gap-2 py-0.5">
    <label className="flex min-h-6 flex-1 cursor-pointer items-center gap-2 text-[11px] text-ink">
      <Check value={value} onChange={onChange} />
      {swatch && <span className="size-2.5 shrink-0 rounded-[2px]" style={{ background: swatch }} />}
      <span className="flex-1">{label}</span>
    </label>
    <button className="rounded px-1.5 py-0.5 text-[10px] text-accent-ink hover:bg-hover" onClick={onOnly} aria-label={`Select only ${label.toLowerCase()}`}>Only</button>
  </div>;
}

function Group({ title, all, onAll, children }: { title: string; all: boolean; onAll: (v: boolean) => void; children: ReactNode }) {
  return <div className="py-1">
    <label className="flex min-h-7 cursor-pointer items-center gap-2 border-b border-line text-[11px] font-semibold text-ink">
      <Check value={all} onChange={onAll} />
      <span className="flex-1">{title}</span>
      <span className="text-[10px] font-normal text-faint">Select all</span>
    </label>
    <div className="pt-0.5">{children}</div>
  </div>;
}

/**
 * Types and layers that clicks and box selection may pick on a board.
 * `compact` (the View tab) shows types only: that tab already lists layers and the ratsnest switch.
 */
export function SelectionFilter({ doc, compact = false }: { doc: DocState; compact?: boolean }) {
  const filter = useSelectFilter();
  const layers = filterLayers(doc);
  const known = layers.map(x => x.id);
  const off = new Set(filter.offLayers);
  return <div>
    <Group title="Types" all={Object.values(filter.on).every(Boolean)} onAll={v => (v ? filter.all() : filter.none())}>
      {FILTER_KINDS.map(([key, label]) => <Row key={key} label={label} value={filter.on[key]} onChange={v => filter.set(key, v)} onOnly={() => filter.only(key)} />)}
    </Group>
    {!compact && <Group title="Layers" all={!known.some(id => off.has(id))} onAll={v => (v ? filter.allLayers() : filter.onlyLayers([], known))}>
      {layers.map(x => <Row key={x.id} label={x.name} swatch={layer(x.id).color} value={!off.has(x.id)} onChange={v => filter.setLayer(x.id, v)} onOnly={() => filter.onlyLayers([x.id], known)} />)}
      <Button className="mt-1" variant="outline" size="sm" onClick={() => filter.onlyLayers([doc.activeLayer], known)}>Active layer only</Button>
    </Group>}
    {!compact && <label className="flex min-h-7 cursor-pointer items-center gap-2 border-t border-line pt-1 text-[11px] text-ink">
      <Check value={doc.view.airwires} onChange={airwires => useEditor.getState().setView(doc.info.id, { airwires })} />
      <span className="flex-1">Show ratsnest</span>
      <span className="text-[10px] text-faint">display</span>
    </label>}
    <div className="flex items-center justify-between gap-2 pt-1">
      <span className="text-[10px] text-faint">{filterActive(filter) ? 'Filtered' : 'Everything selectable'}</span>
      <Button variant="outline" size="sm" onClick={filter.reset}>Reset</Button>
    </div>
    <p className="pt-2 text-[11px] text-muted-foreground">{compact
      ? 'Objects on hidden layers (Layers below) cannot be picked either. Applies to clicks and box selection: left to right selects enclosed objects, right to left also touching ones.'
      : 'An object is picked only when its type and its layer are both enabled. Components follow the copper layer of their side; vias match any copper layer they span. Applies to clicks and box selection: left to right selects enclosed objects, right to left also touching ones.'}</p>
  </div>;
}
