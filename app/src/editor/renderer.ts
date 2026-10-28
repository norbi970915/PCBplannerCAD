import type { Coord, Point } from "../core/types";
import { NM_PER_MM } from "../core/units";
import type { Camera } from "./camera";
import { HighlightTexture } from "./highlightTexture";

export type Rgba = [number, number, number, number];

export interface DrawLine {
  a: Point;
  b: Point;
  width: Coord;
  color: Rgba;
  style?: number;
}

export interface DrawFill {
  /** Flat triangle list [x,y, x,y, x,y, …] in nanometers. */
  tris: number[];
  color: Rgba;
  style?: number;
}

/** Primitives painted together; groups paint in ascending rank. */
export interface DrawGroup {
  rank: number;
  lines: DrawLine[];
  fills: DrawFill[];
}

export interface GridStyle {
  step: Coord;
  background: Rgba;
  dot: Rgba;
  major: Rgba;
  axis: Rgba;
}

const FILL_VS = `#version 300 es
layout(location = 0) in vec2 a_pos;
layout(location = 1) in vec4 a_color;
layout(location = 2) in uint a_style;
uniform sampler2D u_highlights;
uniform vec4 u_unmarkedTint;
uniform vec2 u_center;
uniform float u_scale;
uniform vec2 u_res;
out vec4 v_color;
void main() {
  ivec2 size = textureSize(u_highlights, 0);
  int id = int(a_style);
  vec4 tint = texelFetch(u_highlights, ivec2(id % size.x, id / size.x), 0);
  if (id > 0 && tint.a == 0.0) tint = u_unmarkedTint;
  v_color = vec4(mix(a_color.rgb, tint.rgb, tint.a * 0.3), a_color.a);
  gl_Position = vec4((a_pos - u_center) * u_scale / (u_res * 0.5), 0.0, 1.0);
}`;

const FILL_FS = `#version 300 es
precision highp float;
in vec4 v_color;
out vec4 o;
void main() {
  o = vec4(v_color.rgb * v_color.a, v_color.a);
}`;

const GRID_VS = `#version 300 es
void main() {
  vec2 p = vec2(float((gl_VertexID << 1) & 2), float(gl_VertexID & 2));
  gl_Position = vec4(p * 2.0 - 1.0, 0.0, 1.0);
}`;

// Works entirely in device-pixel space; the CPU supplies grid phases so no
// large world coordinates ever reach float32.
const GRID_FS = `#version 300 es
precision highp float;
uniform vec2 u_phase;
uniform float u_minor;
uniform vec2 u_phaseMajor;
uniform float u_major;
uniform vec2 u_origin;
uniform vec4 u_bg, u_dot, u_majorColor, u_axis;
out vec4 o;
void main() {
  vec2 p = gl_FragCoord.xy;
  vec3 col = u_bg.rgb;
  vec2 dm = mod(p - u_phaseMajor, u_major);
  vec2 em = min(dm, u_major - dm);
  col = mix(col, u_majorColor.rgb, (1.0 - smoothstep(0.0, 1.0, min(em.x, em.y))) * u_majorColor.a);
  vec2 d = mod(p - u_phase, u_minor);
  vec2 e = min(d, u_minor - d);
  col = mix(col, u_dot.rgb, (1.0 - smoothstep(0.6, 1.6, length(e))) * u_dot.a);
  vec2 ea = abs(p - u_origin);
  col = mix(col, u_axis.rgb, (1.0 - smoothstep(0.0, 1.0, min(ea.x, ea.y))) * u_axis.a);
  o = vec4(col, 1.0);
}`;

// Each instance is a capsule (round-capped segment) expanded to a quad.
const LINE_VS = `#version 300 es
layout(location = 0) in vec2 a_a;
layout(location = 1) in vec2 a_b;
layout(location = 2) in float a_hw;
layout(location = 3) in vec4 a_color;
layout(location = 4) in uint a_style;
uniform sampler2D u_highlights;
uniform vec4 u_unmarkedTint;
uniform vec2 u_center;
uniform float u_scale;
uniform vec2 u_res;
out vec2 v_local;
out float v_len;
out float v_hw;
out vec4 v_color;
void main() {
  vec2 corner = vec2(float(gl_VertexID & 1), float((gl_VertexID >> 1) & 1)) * 2.0 - 1.0;
  vec2 A = (a_a - u_center) * u_scale;
  vec2 B = (a_b - u_center) * u_scale;
  float hw = max(a_hw * u_scale, 0.5);
  float r = hw + 1.0;
  vec2 dir = B - A;
  float len = length(dir);
  vec2 t = len > 1e-4 ? dir / len : vec2(1.0, 0.0);
  vec2 n = vec2(-t.y, t.x);
  float u = corner.x < 0.0 ? -r : len + r;
  float v = corner.y * r;
  v_local = vec2(u, v);
  v_len = len;
  v_hw = hw;
  ivec2 size = textureSize(u_highlights, 0);
  int id = int(a_style);
  vec4 tint = texelFetch(u_highlights, ivec2(id % size.x, id / size.x), 0);
  if (id > 0 && tint.a == 0.0) tint = u_unmarkedTint;
  v_color = vec4(mix(a_color.rgb, tint.rgb, tint.a), a_color.a);
  gl_Position = vec4((A + t * u + n * v) / (u_res * 0.5), 0.0, 1.0);
}`;

const LINE_FS = `#version 300 es
precision highp float;
in vec2 v_local;
in float v_len;
in float v_hw;
in vec4 v_color;
out vec4 o;
void main() {
  float x = clamp(v_local.x, 0.0, v_len);
  float d = length(vec2(v_local.x - x, v_local.y));
  float a = clamp(v_hw + 0.5 - d, 0.0, 1.0) * v_color.a;
  if (a <= 0.0) discard;
  o = vec4(v_color.rgb * a, a);
}`;

const MIN_GRID_PX = 12;
const FLOATS_PER_INSTANCE = 7; // a.xy, b.xy, halfWidth, rgba8, item style index
const STRIDE = FLOATS_PER_INSTANCE * 4;

function compile(gl: WebGL2RenderingContext, vs: string, fs: string): WebGLProgram {
  const program = gl.createProgram()!;
  for (const [type, src] of [
    [gl.VERTEX_SHADER, vs],
    [gl.FRAGMENT_SHADER, fs],
  ] as const) {
    const shader = gl.createShader(type)!;
    gl.shaderSource(shader, src);
    gl.compileShader(shader);
    if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) throw new Error(`Shader compile failed: ${gl.getShaderInfoLog(shader)}`);
    gl.attachShader(program, shader);
  }
  gl.linkProgram(program);
  if (!gl.getProgramParameter(program, gl.LINK_STATUS)) throw new Error(`Shader link failed: ${gl.getProgramInfoLog(program)}`);
  return program;
}

class LineBatch {
  private vao: WebGLVertexArrayObject;
  private buffer: WebGLBuffer;
  private capacity = 0;
  count = 0;

  constructor(private gl: WebGL2RenderingContext) {
    this.vao = gl.createVertexArray()!;
    this.buffer = gl.createBuffer()!;
    gl.bindVertexArray(this.vao);
    gl.bindBuffer(gl.ARRAY_BUFFER, this.buffer);
    const attr = (loc: number, size: number, offset: number, type: number = gl.FLOAT, normalized = false) => {
      gl.enableVertexAttribArray(loc);
      gl.vertexAttribPointer(loc, size, type, normalized, STRIDE, offset);
      gl.vertexAttribDivisor(loc, 1);
    };
    attr(0, 2, 0);
    attr(1, 2, 8);
    attr(2, 1, 16);
    attr(3, 4, 20, gl.UNSIGNED_BYTE, true);
    gl.enableVertexAttribArray(4);
    gl.vertexAttribIPointer(4, 1, gl.UNSIGNED_INT, STRIDE, 24);
    gl.vertexAttribDivisor(4, 1);
    gl.bindVertexArray(null);
  }

  upload(lines: readonly DrawLine[]) {
    const gl = this.gl;
    const data = new ArrayBuffer(Math.max(1, lines.length) * STRIDE);
    const f = new Float32Array(data);
    const u8 = new Uint8Array(data);
    const u32 = new Uint32Array(data);
    lines.forEach((l, i) => {
      const o = i * FLOATS_PER_INSTANCE;
      f[o] = l.a.x / NM_PER_MM;
      f[o + 1] = l.a.y / NM_PER_MM;
      f[o + 2] = l.b.x / NM_PER_MM;
      f[o + 3] = l.b.y / NM_PER_MM;
      f[o + 4] = l.width / 2 / NM_PER_MM;
      u32[o + 6] = l.style ?? 0;
      const c = (o + 5) * 4;
      for (let k = 0; k < 4; k++) u8[c + k] = Math.round(l.color[k] * 255);
    });
    gl.bindBuffer(gl.ARRAY_BUFFER, this.buffer);
    if (data.byteLength > this.capacity) {
      gl.bufferData(gl.ARRAY_BUFFER, data, gl.DYNAMIC_DRAW);
      this.capacity = data.byteLength;
    } else {
      gl.bufferSubData(gl.ARRAY_BUFFER, 0, data);
    }
    this.count = lines.length;
  }

  draw() {
    if (!this.count) return;
    this.gl.bindVertexArray(this.vao);
    this.gl.drawArraysInstanced(this.gl.TRIANGLE_STRIP, 0, 4, this.count);
  }

  dispose() {
    this.gl.deleteBuffer(this.buffer);
    this.gl.deleteVertexArray(this.vao);
  }
}

const FILL_STRIDE = 16; // x, y as float32 + rgba8 + item style index

class FillBatch {
  private vao: WebGLVertexArrayObject;
  private buffer: WebGLBuffer;
  private capacity = 0;
  count = 0;

  constructor(private gl: WebGL2RenderingContext) {
    this.vao = gl.createVertexArray()!;
    this.buffer = gl.createBuffer()!;
    gl.bindVertexArray(this.vao);
    gl.bindBuffer(gl.ARRAY_BUFFER, this.buffer);
    gl.enableVertexAttribArray(0);
    gl.vertexAttribPointer(0, 2, gl.FLOAT, false, FILL_STRIDE, 0);
    gl.enableVertexAttribArray(1);
    gl.vertexAttribPointer(1, 4, gl.UNSIGNED_BYTE, true, FILL_STRIDE, 8);
    gl.enableVertexAttribArray(2);
    gl.vertexAttribIPointer(2, 1, gl.UNSIGNED_INT, FILL_STRIDE, 12);
    gl.bindVertexArray(null);
  }

  upload(fills: readonly DrawFill[]) {
    const gl = this.gl;
    const vertices = fills.reduce((n, f) => n + f.tris.length / 2, 0);
    const data = new ArrayBuffer(Math.max(1, vertices) * FILL_STRIDE);
    const f32 = new Float32Array(data);
    const u32 = new Uint32Array(data);
    const u8 = new Uint8Array(data);
    let v = 0;
    for (const fill of fills) {
      const c = fill.color.map((x) => Math.round(x * 255));
      for (let i = 0; i < fill.tris.length; i += 2, v++) {
        f32[v * 4] = fill.tris[i] / NM_PER_MM;
        f32[v * 4 + 1] = fill.tris[i + 1] / NM_PER_MM;
        u32[v * 4 + 3] = fill.style ?? 0;
        const o = v * FILL_STRIDE + 8;
        u8[o] = c[0];
        u8[o + 1] = c[1];
        u8[o + 2] = c[2];
        u8[o + 3] = c[3];
      }
    }
    gl.bindBuffer(gl.ARRAY_BUFFER, this.buffer);
    if (data.byteLength > this.capacity) {
      gl.bufferData(gl.ARRAY_BUFFER, data, gl.DYNAMIC_DRAW);
      this.capacity = data.byteLength;
    } else {
      gl.bufferSubData(gl.ARRAY_BUFFER, 0, data);
    }
    this.count = vertices;
  }

  draw() {
    if (!this.count) return;
    this.gl.bindVertexArray(this.vao);
    this.gl.drawArrays(this.gl.TRIANGLES, 0, this.count);
  }

  dispose() {
    this.gl.deleteBuffer(this.buffer);
    this.gl.deleteVertexArray(this.vao);
  }
}

/** GPU buffers for a list of groups; buffers are pooled and reused. */
class GroupSet {
  private pool = new Map<number, { lines: LineBatch; fills: FillBatch; lineData?: readonly DrawLine[]; fillData?: readonly DrawFill[] }>();
  private active: { lines: LineBatch; fills: FillBatch }[] = [];

  constructor(private gl: WebGL2RenderingContext) {}

  /** Submitted arrays are immutable; unchanged batches need no packing or upload. */
  upload(groups: readonly DrawGroup[]) {
    this.active = groups.map(group => {
      let batch = this.pool.get(group.rank);
      if (!batch) this.pool.set(group.rank, batch = { lines: new LineBatch(this.gl), fills: new FillBatch(this.gl) });
      if (batch.fillData !== group.fills) { batch.fills.upload(group.fills); batch.fillData = group.fills; }
      if (batch.lineData !== group.lines) { batch.lines.upload(group.lines); batch.lineData = group.lines; }
      return batch;
    });
  }

  draw(useLines: () => void, useFills: () => void) {
    for (const batch of this.active) {
      if (batch.fills.count) { useFills(); batch.fills.draw(); }
      if (batch.lines.count) { useLines(); batch.lines.draw(); }
    }
  }

  dispose() {
    for (const batch of this.pool.values()) { batch.lines.dispose(); batch.fills.dispose(); }
    this.pool.clear(); this.active = [];
  }
}

export class Renderer {
  private gl: WebGL2RenderingContext;
  private gridProgram: WebGLProgram;
  private lineProgram: WebGLProgram;
  private fillProgram: WebGLProgram;
  private gridVao: WebGLVertexArrayObject;
  private scene: GroupSet;
  private overlay: GroupSet;
  private analysis: GroupSet;
  private highlights: HighlightTexture;
  private dpr = 1;
  private unmarkedTint: Rgba = [0, 0, 0, 0];
  private uniforms = new Map<WebGLProgram, Map<string, WebGLUniformLocation | null>>();

  constructor(private canvas: HTMLCanvasElement) {
    const gl = canvas.getContext("webgl2", { antialias: true, premultipliedAlpha: true, alpha: false, preserveDrawingBuffer: false });
    if (!gl) throw new Error("WebGL2 is not available on this system.");
    this.gl = gl;
    this.gridProgram = compile(gl, GRID_VS, GRID_FS);
    this.lineProgram = compile(gl, LINE_VS, LINE_FS);
    this.fillProgram = compile(gl, FILL_VS, FILL_FS);
    this.gridVao = gl.createVertexArray()!;
    this.scene = new GroupSet(gl);
    this.overlay = new GroupSet(gl);
    this.analysis = new GroupSet(gl);
    this.highlights = new HighlightTexture(gl);
    this.highlights.set(new Map(), 1);
  }

  private u(program: WebGLProgram, name: string) {
    let m = this.uniforms.get(program);
    if (!m) this.uniforms.set(program, (m = new Map()));
    if (!m.has(name)) m.set(name, this.gl.getUniformLocation(program, name));
    return m.get(name)!;
  }

  resize(cssWidth: number, cssHeight: number, dpr: number) {
    this.dpr = dpr;
    this.canvas.width = Math.max(1, Math.round(cssWidth * dpr));
    this.canvas.height = Math.max(1, Math.round(cssHeight * dpr));
  }

  setScene(groups: readonly DrawGroup[]) {
    this.scene.upload(groups);
  }

  setHighlights(styles: ReadonlyMap<number, Rgba>, count: number, unmarkedTint: Rgba = [0, 0, 0, 0]) { this.highlights.set(styles, count); this.unmarkedTint = unmarkedTint; }

  setAnalysis(groups: readonly DrawGroup[]) { this.analysis.upload(groups); }

  setOverlay(groups: readonly DrawGroup[]) {
    this.overlay.upload(groups);
  }

  render(cam: Camera, grid: GridStyle) {
    const gl = this.gl;
    const W = this.canvas.width;
    const H = this.canvas.height;
    gl.viewport(0, 0, W, H);

    // Device px per nm; world origin in GL fragment coordinates (Y up).
    const s = cam.scale * this.dpr;
    const ox = W / 2 - cam.cx * s;
    const oy = H / 2 - cam.cy * s;
    // The visible grid is the snap grid times the smallest 1-2-5 multiple that
    // keeps dots at least MIN_GRID_PX apart; snapping itself is unaffected.
    const snapPx = grid.step * s;
    let minor = snapPx;
    for (let decade = 1; minor < MIN_GRID_PX * this.dpr && decade < 1e9; decade *= 10) {
      for (const m of [1, 2, 5]) {
        minor = snapPx * decade * m;
        if (minor >= MIN_GRID_PX * this.dpr) break;
      }
    }
    const major = minor * 10;
    const mod = (v: number, m: number) => ((v % m) + m) % m;

    gl.disable(gl.BLEND);
    gl.useProgram(this.gridProgram);
    const g = this.gridProgram;
    gl.uniform1f(this.u(g, "u_minor"), minor);
    gl.uniform2f(this.u(g, "u_phase"), mod(ox, minor) + 0.5, mod(oy, minor) + 0.5);
    gl.uniform1f(this.u(g, "u_major"), major);
    gl.uniform2f(this.u(g, "u_phaseMajor"), mod(ox, major) + 0.5, mod(oy, major) + 0.5);
    gl.uniform2f(this.u(g, "u_origin"), Math.round(ox) + 0.5, Math.round(oy) + 0.5);
    gl.uniform4fv(this.u(g, "u_bg"), grid.background);
    gl.uniform4fv(this.u(g, "u_dot"), grid.dot);
    gl.uniform4fv(this.u(g, "u_majorColor"), grid.major);
    gl.uniform4fv(this.u(g, "u_axis"), grid.axis);
    gl.bindVertexArray(this.gridVao);
    gl.drawArrays(gl.TRIANGLES, 0, 3);

    gl.enable(gl.BLEND);
    gl.blendFunc(gl.ONE, gl.ONE_MINUS_SRC_ALPHA);
    for (const p of [this.lineProgram, this.fillProgram]) {
      gl.useProgram(p);
      gl.uniform2f(this.u(p, "u_center"), cam.cx / NM_PER_MM, cam.cy / NM_PER_MM);
      gl.uniform1f(this.u(p, "u_scale"), s * NM_PER_MM);
      gl.uniform2f(this.u(p, "u_res"), W, H);
      gl.uniform4fv(this.u(p, "u_unmarkedTint"), this.unmarkedTint);
    }
    this.highlights.bind();
    let current: WebGLProgram | null = null;
    const use = (p: WebGLProgram) => () => {
      if (current !== p) gl.useProgram((current = p));
    };
    this.scene.draw(use(this.lineProgram), use(this.fillProgram));
    this.analysis.draw(use(this.lineProgram), use(this.fillProgram));
    this.overlay.draw(use(this.lineProgram), use(this.fillProgram));
    gl.bindVertexArray(null);
  }

  dispose() {
    this.scene.dispose();
    this.overlay.dispose();
    this.analysis.dispose();
    this.highlights.dispose();
    this.gl.deleteVertexArray(this.gridVao);
    this.gl.deleteProgram(this.gridProgram);
    this.gl.deleteProgram(this.lineProgram);
    this.gl.deleteProgram(this.fillProgram);
  }
}
