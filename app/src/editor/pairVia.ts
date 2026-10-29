import type { Point, TrackItem, ViaItem } from '../core/types';

/** Widen the pair symmetrically before placing annuli that need more room than tracks. */
export function pairViaFanout(positive: ViaItem, negative: ViaItem, direction: Point, width: number, clearance: number): { tracks: TrackItem[]; vias: ViaItem[] } {
  const dx=negative.pos.x-positive.pos.x, dy=negative.pos.y-positive.pos.y;
  const distance=Math.hypot(dx,dy);
  if(distance<1)throw new Error('Pair endpoints coincide; move to a clear routing position');
  const required=(positive.diameter+negative.diameter)/2+clearance+10;
  if(distance>=required)return {tracks:[],vias:[positive,negative]};
  const sx=dx/distance,sy=dy/distance;
  let fx=-sy,fy=sx;
  if(fx*direction.x+fy*direction.y<0){fx=-fx;fy=-fy;}
  const spread=(required-distance)/2;
  const lead=Math.max(spread, Math.max(positive.diameter,negative.diameter)/2+width/2+clearance+10);
  const at=(p:Point,side:number):Point=>({x:Math.round(p.x+fx*lead+side*sx*spread),y:Math.round(p.y+fy*lead+side*sy*spread)});
  const vias=[{...positive,pos:at(positive.pos,-1)},{...negative,pos:at(negative.pos,1)}];
  const tracks=[positive,negative].map((via,i):TrackItem=>({kind:'track',locked:false,a:via.pos,b:vias[i].pos,width,layer:via.fromLayer,net:via.net}));
  return {tracks,vias};
}
