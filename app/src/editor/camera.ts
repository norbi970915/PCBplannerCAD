import type { Point } from "../core/types";
import { NM_PER_MM } from "../core/units";

export interface Bounds {
  minX: number;
  minY: number;
  maxX: number;
  maxY: number;
}

// Screen pixels per nanometer: from 0.02 px/mm (a 50 m view) to 50 000 px/mm.
const MIN_SCALE = 0.02 / NM_PER_MM;
const MAX_SCALE = 50_000 / NM_PER_MM;

/**
 * Maps world nanometers (Y up, PCB convention) to CSS pixels (Y down).
 * All math is in doubles; only the renderer narrows to float32.
 */
export class Camera {
  cx = 0;
  cy = 0;
  scale = 20 / NM_PER_MM;
  width = 1;
  height = 1;

  worldToScreen(p: Point): Point {
    return { x: this.width / 2 + (p.x - this.cx) * this.scale, y: this.height / 2 - (p.y - this.cy) * this.scale };
  }

  screenToWorld(sx: number, sy: number): Point {
    return { x: this.cx + (sx - this.width / 2) / this.scale, y: this.cy - (sy - this.height / 2) / this.scale };
  }

  zoomAt(sx: number, sy: number, factor: number) {
    const before = this.screenToWorld(sx, sy);
    this.scale = Math.min(MAX_SCALE, Math.max(MIN_SCALE, this.scale * factor));
    const after = this.screenToWorld(sx, sy);
    this.cx += before.x - after.x;
    this.cy += before.y - after.y;
  }

  panBy(dxPx: number, dyPx: number) {
    this.cx -= dxPx / this.scale;
    this.cy += dyPx / this.scale;
  }

  fit(b: Bounds, marginPx = 40) {
    const w = Math.max(b.maxX - b.minX, NM_PER_MM);
    const h = Math.max(b.maxY - b.minY, NM_PER_MM);
    this.cx = (b.minX + b.maxX) / 2;
    this.cy = (b.minY + b.maxY) / 2;
    const sx = Math.max(1, this.width - 2 * marginPx) / w;
    const sy = Math.max(1, this.height - 2 * marginPx) / h;
    this.scale = Math.min(MAX_SCALE, Math.max(MIN_SCALE, Math.min(sx, sy)));
  }
}
