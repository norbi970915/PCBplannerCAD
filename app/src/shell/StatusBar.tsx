import { layer } from "../core/layers";
import { useEditor } from "../core/store";
import { formatLength } from "../core/units";

function Cell({ children, className = "" }: { children: React.ReactNode; className?: string }) {
  return <span className={`flex items-center gap-1.5 border-r border-line px-2.5 last:border-r-0 ${className}`}>{children}</span>;
}

export function StatusBar() {
  const units = useEditor((s) => s.units);
  const cursor = useEditor((s) => s.cursor);
  const pendingKeys = useEditor((s) => s.pendingKeys);
  const hint = useEditor((s) => s.hint);
  const doc = useEditor((s) => (s.activeDocId ? s.docs[s.activeDocId] : undefined));
  const coord = (v: number | undefined) => (v === undefined ? "—" : formatLength(v, units));
  const board = doc && (doc.info.kind === "pcb" || doc.info.kind === "footprint");

  return (
    <div className="flex h-6 shrink-0 items-center border-t border-line bg-chrome text-[11px] text-muted-foreground">
      <Cell className="min-w-56 font-mono text-ink">
        X {coord(cursor?.x)} <span className="text-faint">·</span> Y {coord(cursor?.y)}
      </Cell>
      {doc && <Cell>Grid {formatLength(doc.grid, units)}</Cell>}
      {board && (
        <Cell>
          <span className="swatch" style={{ background: layer(doc.activeLayer).color }} /> {layer(doc.activeLayer).name}
        </Cell>
      )}
      <Cell className="min-w-0 flex-1 truncate">
        {pendingKeys ? (
          <span className="text-accent-ink">
            <kbd className="font-mono">{pendingKeys}</kbd> … waiting for next key
          </span>
        ) : doc ? (
          hint
        ) : (
          "Open or create a project to start"
        )}
      </Cell>
      {doc && (
        <Cell>
          {doc.selection.size ? `${doc.selection.size} selected · ` : ""}
          {doc.items.size} objects
        </Cell>
      )}
      <Cell className="font-mono uppercase text-accent-ink">{units}</Cell>
    </div>
  );
}
