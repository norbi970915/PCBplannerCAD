import { api } from "./api";
import { execute, report } from "./actions";
import { activeDoc, useEditor } from "./store";
import type { CachedPart, DocKind, Item, Point } from "./types";

/** In-app clipboard: items plus the library data pasted parts need. */
interface Clip {
  kind: DocKind;
  items: Item[];
  parts: CachedPart[];
  anchor: Point;
}

let clip: Clip | null = null;

const st = () => useEditor.getState();

export function hasClip() {
  return clip !== null;
}

export function copySelection(): boolean {
  const doc = activeDoc();
  if (!doc || !doc.selection.size) return false;
  const items = [...doc.selection].map((id) => doc.items.get(id)).filter((i): i is Item => !!i);
  const keys = new Set(items.flatMap((i) => (i.kind === "part" ? [i.libRef] : [])));
  const parts = [...keys].map((k) => doc.cache[k]).filter((p): p is CachedPart => !!p);
  clip = { kind: doc.info.kind, items, parts, anchor: st().cursor ?? anchorOf(items) };
  st().log("info", `Copied ${items.length} object${items.length === 1 ? "" : "s"}.`);
  return true;
}

export async function cutSelection() {
  const doc = activeDoc();
  if (doc && copySelection()) await execute(doc.info.id, { op: "delete", ids: [...doc.selection] });
}

/** Starts the paste tool on the active document; click places the copy. */
export async function paste() {
  const doc = activeDoc();
  if (!doc || !clip) return;
  if (clip.kind !== doc.info.kind) {
    st().log("warning", `Can't paste ${clip.kind} objects into a ${doc.info.kind} document.`);
    return;
  }
  try {
    if (clip.parts.length) {
      const cache = await api.putCache(doc.info.id, clip.parts);
      st().patchDoc(doc.info.id, { cache });
    }
    st().setPlacement({ paste: { items: clip.items, anchor: clip.anchor } });
    st().patchDoc(doc.info.id, { tool: "paste", selection: new Set() });
  } catch (e) {
    report(e);
  }
}

export async function duplicateSelection() {
  if (copySelection()) await paste();
}

function anchorOf(items: Item[]): Point {
  const pts = items.flatMap((i): Point[] => {
    if ("pos" in i) return [i.pos];
    if ("a" in i) return [i.a, i.b];
    if ("center" in i) return [i.center];
    if ("points" in i) return i.points;
    return [];
  });
  if (!pts.length) return { x: 0, y: 0 };
  return { x: Math.min(...pts.map((p) => p.x)), y: Math.max(...pts.map((p) => p.y)) };
}
