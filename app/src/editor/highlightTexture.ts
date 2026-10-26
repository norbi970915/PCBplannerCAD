import type { Rgba } from './renderer';

/** Tiny per-item tint table; pointer hover changes texels, never board geometry. */
export class HighlightTexture {
  private texture: WebGLTexture;
  private width = 256;
  private height = 0;
  private current = new Map<number, Uint8Array>();

  constructor(private gl: WebGL2RenderingContext) {
    this.texture = gl.createTexture()!;
    this.bind();
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
  }

  bind() { this.gl.activeTexture(this.gl.TEXTURE0); this.gl.bindTexture(this.gl.TEXTURE_2D, this.texture); }

  set(styles: ReadonlyMap<number, Rgba>, count: number) {
    const gl = this.gl;
    const next = new Map<number, Uint8Array>();
    for (const [id, color] of styles) next.set(id, Uint8Array.from(color, x => Math.round(x * 255)));
    const needed = Math.max(1, Math.ceil(count / this.width));
    this.bind();
    if (needed > this.height) {
      this.height = 2 ** Math.ceil(Math.log2(needed));
      if (this.height > gl.getParameter(gl.MAX_TEXTURE_SIZE)) throw new Error('Highlight table exceeds GPU capacity.');
      const data = new Uint8Array(this.width * this.height * 4);
      for (const [id, bytes] of next) data.set(bytes, id * 4);
      gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA8, this.width, this.height, 0, gl.RGBA, gl.UNSIGNED_BYTE, data);
    } else {
      const upload = (id: number, bytes: Uint8Array) => gl.texSubImage2D(gl.TEXTURE_2D, 0, id % this.width, Math.floor(id / this.width), 1, 1, gl.RGBA, gl.UNSIGNED_BYTE, bytes);
      for (const id of this.current.keys()) if (!next.has(id)) upload(id, new Uint8Array(4));
      for (const [id, bytes] of next) {
        const old = this.current.get(id);
        if (!old || bytes.some((x, i) => x !== old[i])) upload(id, bytes);
      }
    }
    this.current = next;
  }

  dispose() { this.gl.deleteTexture(this.texture); }
}
