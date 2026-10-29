// Loads footprint 3D models (STEP via OpenCascade/WASM, VRML via three.js)
// into footprint-local millimeter space.

import * as THREE from "three";
import { VRMLLoader } from "three/examples/jsm/loaders/VRMLLoader.js";
import wasmUrl from "occt-import-js/dist/occt-import-js.wasm?url";
import { api } from "../core/api";
import type { Model3d } from "../core/types";

interface OcctMesh {
  name?: string;
  color?: [number, number, number];
  attributes: { position: { array: number[] }; normal?: { array: number[] } };
  index: { array: number[] };
}
interface Occt {
  ReadStepFile(bytes: Uint8Array, params: null): { success: boolean; meshes: OcctMesh[] };
}

let occt: Promise<Occt> | null = null;
function getOcct(): Promise<Occt> {
  occt ??= import("virtual:pcad-step-bindings").then((m) => (m.default as (o: object) => Promise<Occt>)({ locateFile: () => wasmUrl }));
  return occt;
}

/** Parsed models by resolved path; clones are handed out per placement. */
const cache = new Map<string, Promise<THREE.Object3D | null>>();

export async function readModelAsset(path: string, thumbnail = false): Promise<THREE.Object3D | null> {
  const bytes = new Uint8Array(await api.readFileBytes(path));
  const lower = path.toLowerCase();
  if (lower.endsWith(".wrl")) {
    const text = new TextDecoder().decode(bytes);
    const loader=new VRMLLoader();
    if(thumbnail){const manager=new THREE.LoadingManager();manager.setURLModifier(()=>"data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAusB9Wl6iCEAAAAASUVORK5CYII=");loader.manager=manager;}
    const scene = loader.parse(text, "");
    // Imported VRML models use 0.1 inch units.
    scene.scale.setScalar(2.54);
    const root = new THREE.Group();
    root.add(scene);
    return root;
  }
  const lib = await getOcct();
  const res = lib.ReadStepFile(bytes, null);
  if (!res.success) return null;
  const root = new THREE.Group();
  for (const m of res.meshes) {
    const geo = new THREE.BufferGeometry();
    geo.setAttribute("position", new THREE.Float32BufferAttribute(m.attributes.position.array, 3));
    if (m.attributes.normal) geo.setAttribute("normal", new THREE.Float32BufferAttribute(m.attributes.normal.array, 3));
    geo.setIndex(m.index.array);
    if (!m.attributes.normal) geo.computeVertexNormals();
    const color = m.color ? new THREE.Color(m.color[0], m.color[1], m.color[2]) : new THREE.Color(0.25, 0.25, 0.27);
    root.add(new THREE.Mesh(geo, new THREE.MeshStandardMaterial({ color, metalness: 0.15, roughness: 0.55 })));
  }
  return root;
}

/** Resolves and loads a model; null when the file can't be found or parsed. */
export async function loadModel(model: Model3d, baseDir: string | null): Promise<THREE.Object3D | null> {
  const resolved = await api.resolveModelPath(model.path, baseDir);
  if (!resolved) return null;
  let p = cache.get(resolved);
  if (!p) {
    p = readModelAsset(resolved).catch((e) => {
      console.warn("3D model failed", resolved, e);
      return null;
    });
    cache.set(resolved, p);
  }
  const obj = await p;
  if (!obj) return null;
  // Imported-model convention: T(offset) · Rz(-z) · Ry(-y) · Rx(-x) · S.
  const holder = new THREE.Group();
  const inner = obj.clone(true);
  holder.add(inner);
  const d = THREE.MathUtils.degToRad;
  const m = new THREE.Matrix4()
    .makeTranslation(model.offset[0], model.offset[1], model.offset[2])
    .multiply(new THREE.Matrix4().makeRotationZ(-d(model.rotation[2])))
    .multiply(new THREE.Matrix4().makeRotationY(-d(model.rotation[1])))
    .multiply(new THREE.Matrix4().makeRotationX(-d(model.rotation[0])))
    .multiply(new THREE.Matrix4().makeScale(model.scale[0] || 1, model.scale[1] || 1, model.scale[2] || 1));
  holder.matrixAutoUpdate = false;
  holder.matrix.copy(m);
  return holder;
}
