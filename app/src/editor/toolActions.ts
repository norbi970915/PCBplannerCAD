import { api } from "../core/api";
import { report } from "../core/actions";
import { activeDoc, useEditor } from "../core/store";
import { LAYER_MODES } from "../core/view";
import { toolButton } from "./profiles";

const st = () => useEditor.getState();

/** Activates an Active Bar tool (by button id) on the active document. */
export function activateTool(buttonId: string, docId = st().activeDocId) {
  const doc = docId ? st().docs[docId] : undefined;
  if (!doc || !docId) return;
  const btn = toolButton(doc.info.kind, buttonId);
  if (!btn) return;
  // Each tool starts unrotated unless its preset says otherwise (GND points down).
  st().setPlacement({ rotation: 0, mirror: false, ...btn.preset });
  if (btn.tool === "part" && !st().placement.part) {
    const picked = st().pickedComponent;
    if (picked) {
      void startPlacePart(picked.library, picked.component);
      return;
    }
    st().log("warning", "Pick a component in the Components panel first, then press Place.");
    return;
  }
  const keepsSelection = ["select", "reshape", "polygonCut", "polygonSlice"].includes(buttonId);
  st().patchDoc(docId, { tool: buttonId, selection: keepsSelection ? doc.selection : new Set() });
}

/** Caches a library component in the active schematic and starts placing it. */
export async function startPlacePart(library: string, component: string) {
  const doc = activeDoc();
  if (doc?.info.kind === "pcb") return startPlaceFootprint(library, component);
  if (!doc || doc.info.kind !== "schematic") {
    st().log("warning", "Open a schematic sheet or a board to place parts.");
    return;
  }
  try {
    const [key, part] = await api.cacheComponent(doc.info.id, library, component);
    st().setCache(doc.info.id, key, part);
    st().setPlacement({ part: { library, component, key, unit: 1, designator: null }, rotation: 0, mirror: false });
    st().patchDoc(doc.info.id, { tool: "part", selection: new Set() });
  } catch (e) {
    report(e);
  }
}

/** Places a component's default footprint directly on the active board. */
export async function startPlaceFootprint(library: string, component: string) {
  const doc = activeDoc();
  if (!doc || doc.info.kind !== "pcb") return;
  try {
    const part = await api.cachedPart(library, component);
    const ref = part.component.footprints[0];
    if (!ref) {
      st().log("warning", `${component} has no footprint assigned — add one in its library.`);
      return;
    }
    const [key, fp] = await api.pcbCacheFootprint(doc.info.id, library, ref);
    st().setFootprint(doc.info.id, key, fp);
    const value = part.component.parameters.find((p) => p.name.toLowerCase() === "value")?.value || part.component.name;
    const prefix = part.symbol.prefix || "U";
    let next = 1;
    for (const it of doc.items.values()) {
      const m = it.kind === "footprint" && it.designator.startsWith(prefix) ? /^\d+$/.exec(it.designator.slice(prefix.length)) : null;
      if (m) next = Math.max(next, Number(m[0]) + 1);
    }
    st().setPlacement({
      footprint: { key, designator: `${prefix}${next}`, value, component: `${part.library}/${part.component.name}` },
      rotation: 0,
      mirror: false,
    });
    st().patchDoc(doc.info.id, { tool: "footprint", selection: new Set() });
  } catch (e) {
    report(e);
  }
}

/** Shift+S: all layers → other layers dimmed → active layer only → all layers. */
export function cycleLayerMode(docId: string) {
  const d = st().docs[docId];
  if (!d) return;
  const layerMode = LAYER_MODES[d.layerMode].next;
  st().patchDoc(docId, { layerMode });
  st().setHint(`Layer display: ${LAYER_MODES[layerMode].label} (Shift+S cycles)`);
}
