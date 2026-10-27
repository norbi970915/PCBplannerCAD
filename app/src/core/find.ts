// Edit ▸ Find (Ctrl+F): footprints / parts by designator, value or footprint, nets, labels and texts.

import type { Item, ItemId, Point } from "./types";

export type FindKind = "footprint" | "part" | "net" | "label" | "text";

export interface FindMatch {
  kind: FindKind;
  /** What matched, e.g. "R12" or "GND". */
  label: string;
  detail: string;
  ids: ItemId[];
  /** Where to look; nets have none (zoom to the selection instead). */
  pos: Point | null;
  /** Lower is better: exact, prefix, then substring. */
  rank: number;
}

/** Case-insensitive matcher: plain text matches anywhere; `*` and `?` make it a whole-string wildcard. */
export function matcher(query: string): ((s: string) => number | null) | null {
  const q = query.trim().toLowerCase();
  if (!q) return null;
  if (/[*?]/.test(q)) {
    const re = new RegExp(`^${q.replace(/[.+^${}()|[\]\\]/g, "\\$&").replace(/\*/g, ".*").replace(/\?/g, ".")}$`);
    return (s) => (re.test(s.toLowerCase()) ? 1 : null);
  }
  return (s) => {
    const t = s.toLowerCase();
    if (t === q) return 0;
    if (t.startsWith(q)) return 1;
    return t.includes(q) ? 2 : null;
  };
}

const natural = new Intl.Collator(undefined, { numeric: true, sensitivity: "base" });

export function findMatches(items: ReadonlyMap<ItemId, Item>, query: string, limit = 200): FindMatch[] {
  const m = matcher(query);
  if (!m) return [];
  const out: FindMatch[] = [];
  const nets = new Map<string, ItemId[]>();
  const net = (name: string, id: ItemId) => {
    if (!name) return;
    const list = nets.get(name) ?? [];
    if (list[list.length - 1] !== id) list.push(id);
    nets.set(name, list);
  };
  const best = (...fields: [string, string][]) => {
    let hit: { rank: number; field: string; value: string } | null = null;
    for (const [field, value] of fields) {
      const r = value ? m(value) : null;
      if (r !== null && (!hit || r < hit.rank)) hit = { rank: r, field, value };
    }
    return hit;
  };
  for (const [id, it] of items) {
    switch (it.kind) {
      case "footprint": {
        const hit = best(["designator", it.designator], ["value", it.value], ["footprint", it.footprint]);
        if (hit) out.push({ kind: "footprint", label: it.designator || "(no designator)", detail: [it.value, it.footprint.split("/").pop(), it.side === "bottom" ? "bottom" : ""].filter(Boolean).join(" · "), ids: [id], pos: it.pos, rank: hit.rank + (hit.field === "designator" ? 0 : 0.5) });
        for (const n of Object.values(it.padNets)) net(n, id);
        break;
      }
      case "part": {
        const field = (name: string) => it.fields.find((f) => f.name === name)?.value ?? "";
        const hit = best(["designator", field("Designator")], ["value", field("Value")]);
        if (hit) out.push({ kind: "part", label: field("Designator") || "(no designator)", detail: [field("Value"), it.libRef.split("/").pop()].filter(Boolean).join(" · "), ids: [id], pos: it.pos, rank: hit.rank + (hit.field === "designator" ? 0 : 0.5) });
        break;
      }
      case "track":
      case "arcTrack":
      case "via":
      case "zone":
        net(it.net, id);
        break;
      case "netLabel": {
        const r = m(it.text);
        if (r !== null) out.push({ kind: "label", label: it.text, detail: `${it.scope} label`, ids: [id], pos: it.pos, rank: r + 1 });
        break;
      }
      case "powerPort": {
        const r = m(it.net);
        if (r !== null) out.push({ kind: "label", label: it.net, detail: "power port", ids: [id], pos: it.pos, rank: r + 1 });
        break;
      }
      case "text": {
        const r = m(it.text);
        if (r !== null) out.push({ kind: "text", label: it.text, detail: "text", ids: [id], pos: it.pos, rank: r + 2 });
        break;
      }
    }
  }
  for (const [name, ids] of nets) {
    const r = m(name);
    if (r !== null) out.push({ kind: "net", label: name, detail: `net · ${ids.length} objects`, ids, pos: null, rank: r + 0.25 });
  }
  return out.sort((a, b) => a.rank - b.rank || natural.compare(a.label, b.label)).slice(0, limit);
}
