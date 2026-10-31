import { L } from "../core/layers";
import type { Item, Point } from "../core/types";
import { arcPoints, circlePoints, triangulateWithHoles } from "./shapes";

const close = (a: Point, b: Point) => Math.abs(a.x-b.x)<=1000 && Math.abs(a.y-b.y)<=1000;
const area = (ring: Point[]) => Math.abs(ring.reduce((sum,p,i)=>{
  const q=ring[(i+1)%ring.length]; return sum+p.x*q.y-q.x*p.y;
},0));
function inside(p: Point, ring: Point[]) {
  let hit=false;
  for(let i=0,j=ring.length-1;i<ring.length;j=i++) {
    const a=ring[i],b=ring[j];
    if((a.y>p.y)!==(b.y>p.y) && p.x<(b.x-a.x)*(p.y-a.y)/(b.y-a.y)+a.x) hit=!hit;
  }
  return hit;
}

/** Display-only material mask. Open outlines never invent a board surface. */
export function boardSurface(items: Iterable<Item>): number[] {
  const rings: Point[][]=[], chains: Point[][]=[];
  for(const item of items) {
    if(!("layer" in item) || item.layer!==L.boardOutline) continue;
    switch(item.kind) {
      case "line": chains.push([item.a,item.b]); break;
      case "arc": chains.push(arcPoints(item.center,item.radius,item.startAngle,item.sweep)); break;
      case "circle": if(item.radius>0) rings.push(circlePoints(item.center,item.radius,180)); break;
      case "rect": rings.push([item.a,{x:item.b.x,y:item.a.y},item.b,{x:item.a.x,y:item.b.y}]); break;
      case "polyline": if(item.points.length>=2) (item.closed?rings:chains).push([...item.points]); break;
    }
  }
  while(chains.length) {
    const chain=chains.pop()!;
    for(;;) {
      const end=chain[chain.length-1];
      if(chain.length>2 && close(chain[0],end)) {chain.pop();rings.push(chain);break;}
      const next=chains.map((c,i)=>close(c[0],end)||close(c[c.length-1],end)?i:-1).filter(i=>i>=0);
      if(next.length!==1) break;
      const c=chains.splice(next[0],1)[0];
      if(!close(c[0],end)) c.reverse();
      chain.push(...c.slice(1));
    }
  }
  const loops=rings.filter(r=>r.length>=3&&area(r)>0).sort((a,b)=>area(b)-area(a));
  // Containment parity handles disconnected boards, cutouts and nested islands.
  const parent=loops.map((r,i)=>{
    for(let j=i-1;j>=0;j--) if(inside(r[0],loops[j])) return j;
    return -1;
  });
  const depth: number[]=[];
  const triangles: number[]=[];
  loops.forEach((ring,i)=>{
    depth[i]=parent[i]<0?0:depth[parent[i]]+1;
    if(depth[i]%2===0) triangles.push(...triangulateWithHoles(ring,loops.filter((_,j)=>parent[j]===i)));
  });
  return triangles;
}

/** Outline identities stay stable during copper edits, so triangulation is cached. */
export class BoardSurface {
  private outlines: Item[]=[];
  private triangles: number[]=[];
  read(items: Iterable<Item>): number[] {
    const outlines=Array.from(items).filter(i=>"layer" in i&&i.layer===L.boardOutline);
    if(outlines.length!==this.outlines.length || outlines.some((item,i)=>item!==this.outlines[i])) {
      this.outlines=outlines;
      this.triangles=boardSurface(outlines);
    }
    return this.triangles;
  }
}