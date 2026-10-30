// Loads supported local VRML footprint models; other references remain available for exports.
// Supported geometry is converted into footprint-local millimeter space.

import * as THREE from "three";
import { VRMLLoader } from "three/examples/jsm/loaders/VRMLLoader.js";
import { api } from "../core/api";
import type { Model3d } from "../core/types";

/** Parsed models by resolved path; clones are handed out per placement. */
const cache = new Map<string, Promise<THREE.Object3D | null>>();

export async function readModelAsset(path: string, _thumbnail = false): Promise<THREE.Object3D | null> {
  if (!path.toLowerCase().endsWith(".wrl")) return null;
  const bytes = new Uint8Array(await api.readFileBytes(path));
  const manager = new THREE.LoadingManager();
  // Preserve local processing; external texture URLs are not fetched.
  manager.setURLModifier(() => "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAusB9Wl6iCEAAAAASUVORK5CYII=");
  const loader = new VRMLLoader(manager);
  const scene = loader.parse(new TextDecoder().decode(bytes), "");
  scene.scale.setScalar(2.54);
  const root = new THREE.Group(); root.add(scene); return root;
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
