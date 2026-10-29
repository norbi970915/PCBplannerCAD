import { useEffect, useRef, useState } from "react";
import * as THREE from "three";
import { OrbitControls } from "three/examples/jsm/controls/OrbitControls.js";
import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import { Box, Download, Eye, EyeOff, RefreshCw, Rotate3d } from "lucide-react";
import { Button } from "@/components/ui/button";
import { api } from "../core/api";
import { report } from "../core/actions";
import { useEditor } from "../core/store";
import { useTheme } from "../core/theme";
import type { BoardModel3d, Mesh3d } from "../core/types";
import { loadModel } from "./models";
import { paintFaces } from "./textures";

const FR4 = new THREE.Color("#c9b98a");
const dirOf = (p: string | null) => (p ? p.replace(/[\\/][^\\/]*$/, "") : null);

interface View {
  renderer: THREE.WebGLRenderer;
  scene: THREE.Scene;
  camera: THREE.PerspectiveCamera;
  controls: OrbitControls;
  board: THREE.Group;
  parts: THREE.Group;
  render: () => void;
  size: number;
  center: THREE.Vector3;
}

export function View3DPanel({ docId }: { docId: string }) {
  const host = useRef<HTMLDivElement>(null);
  const view = useRef<View | null>(null);
  const [status, setStatus] = useState("Loading…");
  const [showParts, setShowParts] = useState(true);
  const [seq, setSeq] = useState(0);
  const title = useEditor((s) => s.docs[docId]?.info.title ?? "Board");
  const dark = useTheme((s) => s.resolved) === "dark";

  // Renderer lifetime.
  useEffect(() => {
    const el = host.current!;
    const viewWindow=el.ownerDocument.defaultView??window;
    const renderer = new THREE.WebGLRenderer({ antialias: true, preserveDrawingBuffer: true });
    renderer.setPixelRatio(viewWindow.devicePixelRatio||1);
    renderer.outputColorSpace = THREE.SRGBColorSpace;
    el.appendChild(renderer.domElement);
    const scene = new THREE.Scene();
    scene.add(new THREE.HemisphereLight(0xffffff, 0x404048, 1.6));
    const sun = new THREE.DirectionalLight(0xffffff, 1.8);
    sun.position.set(60, -80, 160);
    scene.add(sun);
    const back = new THREE.DirectionalLight(0xffffff, 0.6);
    back.position.set(-60, 80, -120);
    scene.add(back);
    const camera = new THREE.PerspectiveCamera(35, 1, 0.1, 10000);
    camera.up.set(0, 0, 1);
    const controls = new OrbitControls(camera, renderer.domElement);
    controls.enableDamping = false;
    controls.screenSpacePanning = true;
    const board = new THREE.Group();
    const parts = new THREE.Group();
    scene.add(board, parts);
    let frame = 0;
    const render = () => {
      if (frame) return;
      frame = viewWindow.requestAnimationFrame(() => {
        frame = 0;
        renderer.render(scene, camera);
      });
    };
    controls.addEventListener("change", render);
    const resize = () => {
      renderer.setPixelRatio(viewWindow.devicePixelRatio||1);
      const w = Math.max(1, el.clientWidth);
      const h = Math.max(1, el.clientHeight);
      renderer.setSize(w, h, false);
      renderer.domElement.style.width = "100%";
      renderer.domElement.style.height = "100%";
      camera.aspect = w / h;
      camera.updateProjectionMatrix();
      render();
    };
    const ro = new viewWindow.ResizeObserver(resize);
    ro.observe(el);
    viewWindow.addEventListener("resize",resize);
    view.current = { renderer, scene, camera, controls, board, parts, render, size: 100, center: new THREE.Vector3() };
    return () => {
      ro.disconnect();
      viewWindow.removeEventListener("resize",resize);
      viewWindow.cancelAnimationFrame(frame);
      controls.dispose();
      scene.traverse((o) => {
        if (o instanceof THREE.Mesh) {
          o.geometry.dispose();
          for (const m of [o.material].flat()) {
            (m as THREE.MeshStandardMaterial).map?.dispose();
            m.dispose();
          }
        }
      });
      renderer.dispose();
      renderer.domElement.remove();
      view.current = null;
    };
  }, []);

  useEffect(() => {
    const v = view.current;
    if (!v) return;
    v.scene.background = new THREE.Color(dark ? "#1d2026" : "#e6eaef");
    v.render();
  }, [dark]);

  useEffect(() => {
    const v = view.current;
    if (!v) return;
    v.parts.visible = showParts;
    v.render();
  }, [showParts]);

  // (Re)build the board whenever asked.
  useEffect(() => {
    let cancelled = false;
    const v = view.current;
    if (!v) return;
    void (async () => {
      setStatus("Building board…");
      let model: BoardModel3d;
      try {
        model = await api.outBoardModel(docId);
      } catch (e) {
        setStatus(String(e));
        return;
      }
      if (cancelled) return;
      clear(v.board);
      clear(v.parts);
      buildBoard(v, model, docId);
      if (seq === 0) fit(v, "iso");
      v.render();

      const doc = useEditor.getState().docs[docId];
      const base = dirOf(doc?.info.path ?? null) ?? dirOf(useEditor.getState().project?.path ?? null);
      let loaded = 0;
      let missing = 0;
      const total = model.components.length;
      for (const c of model.components) {
        if (cancelled) return;
        const holder = new THREE.Group();
        holder.matrixAutoUpdate = false;
        holder.matrix.fromArray(c.transform);
        holder.name = c.designator;
        const obj = c.model ? await loadModel(c.model, base).catch(() => null) : null;
        if (cancelled) return;
        // Holes and fiducials have no body; a placeholder box would cover them.
        const bodiless = /MountingHole|Fiducial|TestPoint/i.test(c.footprint);
        if (obj) holder.add(obj);
        else if (!bodiless) holder.add(fallbackBox(c.fallbackBox));
        if (!obj && !bodiless) missing++;
        v.parts.add(holder);
        loaded++;
        if (loaded % 8 === 0 || loaded === total) {
          setStatus(`Components ${loaded}/${total}`);
          v.render();
        }
      }
      v.render();
      const warn = model.outlineFound ? "" : " · no board outline (placeholder shown)";
      setStatus(`${total} components${missing ? ` · ${missing} without 3D model (boxes)` : ""}${warn}`);
    })();
    return () => {
      cancelled = true;
    };
  }, [docId, seq]);

  const exportStep = async () => {
    const v = view.current;
    if (!v) return;
    const path = await saveDialog({ filters: [{ name: "STEP", extensions: ["step", "stp"] }, { name: "Wavefront OBJ", extensions: ["obj"] }], defaultPath: title.replace(/\.\w+$/, ".step") });
    if (!path) return;
    try {
      setStatus("Exporting…");
      await api.outStep(path, title.replace(/\.\w+$/, ""), collectMeshes(v));
      setStatus(`Exported ${path}`);
    } catch (e) {
      report(e);
    }
  };

  const btn = "h-6 gap-1 px-2 text-[11px]";
  return (
    <div className="flex h-full flex-col bg-panel">
      <div className="flex items-center gap-1 border-b border-line bg-panel-head px-2 py-1">
        <Button variant="ghost" className={btn} onClick={() => view.current && fit(view.current, "iso")} title="Isometric">
          <Rotate3d /> Iso
        </Button>
        <Button variant="ghost" className={btn} onClick={() => view.current && fit(view.current, "top")}>
          Top
        </Button>
        <Button variant="ghost" className={btn} onClick={() => view.current && fit(view.current, "bottom")}>
          Bottom
        </Button>
        <Button variant="ghost" className={btn} onClick={() => setShowParts(!showParts)}>
          {showParts ? <Eye /> : <EyeOff />} Components
        </Button>
        <Button variant="ghost" className={btn} onClick={() => setSeq(seq + 1)} title="Rebuild from the current board">
          <RefreshCw /> Refresh
        </Button>
        <Button variant="ghost" className={btn} onClick={() => void exportStep()}>
          <Download /> Export STEP
        </Button>
        <span className="ml-auto truncate font-mono text-[11px] text-faint">
          <Box className="mr-1 inline size-3" />
          {status}
        </span>
      </div>
      <div ref={host} className="relative min-h-0 flex-1 overflow-hidden" />
    </div>
  );
}

function clear(g: THREE.Group) {
  for (const c of [...g.children]) {
    c.traverse((o) => {
      if (o instanceof THREE.Mesh) {
        o.geometry.dispose();
        for (const m of [o.material].flat()) {
          (m as THREE.MeshStandardMaterial).map?.dispose();
          m.dispose();
        }
      }
    });
    g.remove(c);
  }
}

function buildBoard(v: View, model: BoardModel3d, docId: string) {
  const pos = model.board.positions;
  const idx = model.board.indices;
  const t = model.thicknessMm;
  let minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity;
  for (let i = 0; i < pos.length; i += 3) {
    minX = Math.min(minX, pos[i]);
    maxX = Math.max(maxX, pos[i]);
    minY = Math.min(minY, pos[i + 1]);
    maxY = Math.max(maxY, pos[i + 1]);
  }
  if (!Number.isFinite(minX)) return;
  const w = maxX - minX || 1;
  const h = maxY - minY || 1;

  // Split triangles into top face, bottom face and edges (flat shading).
  const top: number[] = [];
  const bottom: number[] = [];
  const side: number[] = [];
  const eps = 1e-4;
  for (let i = 0; i < idx.length; i += 3) {
    const tri = [idx[i], idx[i + 1], idx[i + 2]];
    const zs = tri.map((k) => pos[k * 3 + 2]);
    const dst = zs.every((z) => Math.abs(z - t) < eps) ? top : zs.every((z) => Math.abs(z) < eps) ? bottom : side;
    for (const k of tri) dst.push(pos[k * 3], pos[k * 3 + 1], pos[k * 3 + 2]);
  }
  const geometry = (flat: number[], uv: boolean) => {
    const g = new THREE.BufferGeometry();
    g.setAttribute("position", new THREE.Float32BufferAttribute(flat, 3));
    if (uv) {
      const uvs: number[] = [];
      for (let i = 0; i < flat.length; i += 3) uvs.push((flat[i] - minX) / w, (flat[i + 1] - minY) / h);
      g.setAttribute("uv", new THREE.Float32BufferAttribute(uvs, 2));
    }
    g.computeVertexNormals();
    return g;
  };

  const doc = useEditor.getState().docs[docId];
  const faces = doc ? paintFaces(doc, { minX: minX * 1e6, minY: minY * 1e6, maxX: maxX * 1e6, maxY: maxY * 1e6 }) : null;
  const tex = (c: HTMLCanvasElement | undefined) => {
    if (!c) return null;
    const tx = new THREE.CanvasTexture(c);
    tx.colorSpace = THREE.SRGBColorSpace;
    tx.anisotropy = v.renderer.capabilities.getMaxAnisotropy();
    return tx;
  };
  const mask = new THREE.Color(...model.board.color);
  const faceMat = (c: HTMLCanvasElement | undefined) => {
    const map = tex(c);
    return new THREE.MeshStandardMaterial({ color: map ? 0xffffff : mask, map, roughness: 0.55, metalness: 0.05 });
  };
  v.board.add(new THREE.Mesh(geometry(top, true), faceMat(faces?.top.canvas)));
  v.board.add(new THREE.Mesh(geometry(bottom, true), faceMat(faces?.bottom.canvas)));
  v.board.add(new THREE.Mesh(geometry(side, false), new THREE.MeshStandardMaterial({ color: FR4, roughness: 0.8 })));
  v.size = Math.max(w, h, t);
  v.center.set((minX + maxX) / 2, (minY + maxY) / 2, t / 2);
}

function fallbackBox(b: [number, number, number, number, number, number]): THREE.Object3D {
  const [x0, y0, z0, x1, y1, z1] = b;
  const geo = new THREE.BoxGeometry(Math.max(0.05, x1 - x0), Math.max(0.05, y1 - y0), Math.max(0.05, z1 - z0));
  const mesh = new THREE.Mesh(geo, new THREE.MeshStandardMaterial({ color: 0x3a3d44, roughness: 0.7 }));
  mesh.position.set((x0 + x1) / 2, (y0 + y1) / 2, (z0 + z1) / 2);
  return mesh;
}

function fit(v: View, mode: "iso" | "top" | "bottom") {
  const d = v.size * 1.25;
  const c = v.center;
  if (mode === "top") v.camera.position.set(c.x, c.y - d * 0.001, c.z + d);
  else if (mode === "bottom") v.camera.position.set(c.x, c.y - d * 0.001, c.z - d);
  else v.camera.position.set(c.x + d * 0.45, c.y - d * 0.75, c.z + d * 0.7);
  v.camera.near = Math.max(0.05, v.size / 2000);
  v.camera.far = v.size * 50;
  v.camera.updateProjectionMatrix();
  v.controls.target.copy(c);
  v.controls.update();
  v.render();
}

/** Everything visible, in board millimeters, for STEP/OBJ export. */
function collectMeshes(v: View): Mesh3d[] {
  const out: Mesh3d[] = [];
  v.scene.updateMatrixWorld(true);
  const add = (o: THREE.Object3D, name: string) => {
    o.traverse((m) => {
      if (!(m instanceof THREE.Mesh) || !m.visible) return;
      const g = m.geometry as THREE.BufferGeometry;
      const p = g.getAttribute("position");
      if (!p) return;
      const v3 = new THREE.Vector3();
      const positions: number[] = [];
      for (let i = 0; i < p.count; i++) {
        v3.fromBufferAttribute(p, i).applyMatrix4(m.matrixWorld);
        positions.push(v3.x, v3.y, v3.z);
      }
      const indices = g.index ? Array.from(g.index.array as ArrayLike<number>) : [...Array(p.count).keys()];
      // Mirrored transforms (bottom side) flip triangle winding.
      if (m.matrixWorld.determinant() < 0) for (let i = 0; i < indices.length; i += 3) [indices[i + 1], indices[i + 2]] = [indices[i + 2], indices[i + 1]];
      const mat = [m.material].flat()[0] as THREE.MeshStandardMaterial;
      const c = mat.map ? new THREE.Color("#2b6b35") : mat.color;
      out.push({ name, positions, indices, color: [c.r, c.g, c.b] });
    });
  };
  add(v.board, "Board");
  if (v.parts.visible) for (const part of v.parts.children) add(part, part.name || "Component");
  return out;
}
