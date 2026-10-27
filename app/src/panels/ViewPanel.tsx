import { Check } from '@/components/app/fields';
import { Button } from '@/components/ui/button';
import { Section } from '@/components/app/panel';
import { boardCopper, LAYERS, layer } from '../core/layers';
import { useEditor, type DocState } from '../core/store';
import { DEFAULT_VIEW, type ViewSettings } from '../core/view';
import { SelectionFilter } from './SelectionFilter';

export function ViewPanel({ doc }: { doc: DocState }) {
  const view = doc.view;
  const set = (patch: Partial<ViewSettings>) => useEditor.getState().setView(doc.info.id, patch);
  const copper = boardCopper(doc.items.values());
  const copperIds = new Set(copper.map(x => x.id));
  const layers = [...copper, ...LAYERS.filter(x => !copperIds.has(x.id))];
  // Include custom graphic layers present in this document.
  const seen = new Set(layers.map(x => x.id));
  for (const item of doc.items.values()) if ('layer' in item && !seen.has(item.layer)) { layers.push(layer(item.layer)); seen.add(item.layer); }
  const toggle = (key: keyof Omit<ViewSettings, 'hiddenLayers'>, name: string) => (
    <label key={key} className="flex min-h-7 cursor-pointer items-center justify-between gap-3 py-1 text-[11px] text-ink">
      <span>{name}</span><Check value={view[key]} onChange={value => set({ [key]: value })} />
    </label>
  );
  return <>
    {doc.info.kind === 'pcb' && <Section title="Selection filter">
      <SelectionFilter doc={doc} compact />
      <Button className="mt-2" variant="outline" size="sm" onClick={() => useEditor.getState().patchDoc(doc.info.id, { tool: 'select' })}>Select objects</Button>
      <p className="pt-2 text-[11px] text-muted-foreground">Ctrl+click adds or removes objects. Shift+drag adds a box selection; Ctrl+drag toggles it. Ctrl+Shift removes objects from the selection.</p>
    </Section>}
    <Section title="Navigation">
      {toggle('snapToGrid', 'Snap to grid')}
      {toggle('grid', 'Show grid')}
      <p className="pt-1 text-[11px] text-muted-foreground">The idle cursor moves freely. Grid snapping applies while editing when enabled.</p>
    </Section>
    <Section title="Labels">
      {toggle('references', 'Component references')}
      {toggle('values', 'Component values')}
      {toggle('padNumbers', 'Pad numbers')}
      {toggle('padNetNames', 'Pad net names')}
      {toggle('text', 'Other text')}
    </Section>
    <Section title="Objects and markers">
      {toggle('tracks', 'Tracks and arcs')}
      {toggle('vias', 'Vias')}
      {toggle('pads', 'Pads')}
      {toggle('graphics', 'Graphics and outlines')}
      {toggle('airwires', 'Ratsnest (unrouted connections)')}
      {toggle('drc', 'DRC markers')}
    </Section>
    <Section title="Layers">
      <div className="mb-2 flex gap-1.5">
        <Button variant="outline" size="sm" onClick={() => set({ hiddenLayers: [] })}>Show all</Button>
        <Button variant="outline" size="sm" onClick={() => set({ hiddenLayers: layers.filter(x => x.id !== doc.activeLayer && x.id !== 15).map(x => x.id) })}>Active only</Button>
      </div>
      {layers.map(x => <label key={x.id} className="flex min-h-7 cursor-pointer items-center gap-2 py-1 text-[11px]">
        <span className="size-2.5 shrink-0 rounded-[2px]" style={{ background: layer(x.id).color }} />
        <span className="flex-1 text-ink">{x.name}{x.id === doc.activeLayer && <span className="ml-1 text-faint">(active)</span>}</span>
        <Check value={!view.hiddenLayers.includes(x.id)} onChange={visible => set({ hiddenLayers: visible ? view.hiddenLayers.filter(id => id !== x.id) : [...view.hiddenLayers, x.id] })} />
      </label>)}
    </Section>
    <Section title="View preferences">
      <Button variant="outline" size="sm" onClick={() => set({ ...DEFAULT_VIEW, hiddenLayers: [] })}>Reset view</Button>
      <p className="pt-2 text-[11px] text-muted-foreground">Display settings do not change your design or its undo history.</p>
    </Section>
  </>;
}
