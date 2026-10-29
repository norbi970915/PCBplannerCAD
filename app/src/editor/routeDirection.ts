import type {Point} from '../core/types';
/** Sticky screen-distance rails keep pointer noise off horizontal/vertical/45° runs. */
export function snapRouteDirection(from:Point,to:Point,tolerance:number,previous:number|null=null):{point:Point;axis:number|null}{
 const dx=to.x-from.x,dy=to.y-from.y;
 const at=(axis:number)=>{const angle=axis*Math.PI/4,x=Math.cos(angle),y=Math.sin(angle),along=dx*x+dy*y;return{axis,gap:along<0?Infinity:Math.abs(dx*y-dy*x),point:{x:Math.round(from.x+along*x),y:Math.round(from.y+along*y)}};};
 if(previous!==null){const held=at(previous);if(held.gap<=tolerance*1.5)return{point:held.point,axis:previous};}
 let best=at(0);for(let axis=1;axis<8;axis++){const candidate=at(axis);if(candidate.gap<best.gap)best=candidate;}
 return best.gap<=tolerance?{point:best.point,axis:best.axis}:{point:to,axis:null};
}
