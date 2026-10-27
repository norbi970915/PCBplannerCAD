import type { Item, ItemId, Point } from '../core/types';
import { rotatePoint } from '../editor/shapes';

export const ARRAY_LIMIT = 10000;
export interface ArrayOptions { columns:number; rows:number; dx:number; dy:number; keepNets:boolean }
export interface CircularArrayOptions { count:number; center:Point; angle:number; rotate:boolean; keepNets:boolean }
export const canArray = (item:Item) => ['footprint','track','arcTrack','via','zone','line','arc','circle','rect','polyline','text','dimension'].includes(item.kind) && !(item.kind==='zone' && item.teardrop);
type Transform={point:(p:Point)=>Point;rotation:number};
const normalized=(angle:number)=>((angle%360)+360)%360;
function sources(items:ReadonlyMap<ItemId,Item>,ids:Iterable<ItemId>):Item[]{
  const source=[...new Set(ids)].map(id=>{const item=items.get(id);if(!item||!canArray(item))throw Error('Select board objects; settings and generated teardrops cannot be arrayed.');return item;});
  if(!source.length)throw Error('Select the objects to repeat first.');
  return source;
}
/** Reference point for repeating a selection without changing its relative layout. */
export function arrayAnchor(items:ReadonlyMap<ItemId,Item>,ids:Iterable<ItemId>):Point {
  const points:Point[]=[];
  for(const id of ids){const item=items.get(id);if(!item)continue;if('pos'in item)points.push(item.pos);else if('a'in item)points.push(item.a,item.b);else if('center'in item)points.push(item.center);else if(item.kind==='zone'){for(const point of item.outline)points.push(point);}else if(item.kind==='polyline'){for(const point of item.points)points.push(point);}}
  if(!points.length)return{x:0,y:0};
  let minX=Infinity,minY=Infinity,maxX=-Infinity,maxY=-Infinity;
  for(const p of points){minX=Math.min(minX,p.x);minY=Math.min(minY,p.y);maxX=Math.max(maxX,p.x);maxY=Math.max(maxY,p.y);}
  return{x:Math.round((minX+maxX)/2),y:Math.round((minY+maxY)/2)};
}
function copies(items:ReadonlyMap<ItemId,Item>,source:Item[],transforms:Transform[],keepNets:boolean):Item[]{
  if(source.length*transforms.length>ARRAY_LIMIT)throw Error('The array would exceed 10,000 new objects. Reduce its size.');
  const used=new Set([...items.values()].flatMap(i=>i.kind==='footprint'?[i.designator.toUpperCase()]:[]));
  const nextNumber=new Map<string,number>();
  const designator=(original:string)=>{const prefix=original.replace(/[0-9?]+$/,'')||'U',key=prefix.toUpperCase();let n=nextNumber.get(key)??1;while(used.has((prefix+n).toUpperCase()))n++;const name=prefix+n;used.add(name.toUpperCase());nextNumber.set(key,n+1);return name;};
  const out:Item[]=[];
  for(const transform of transforms){
    const point=(p:Point):Point=>{const q=transform.point(p);if(!Number.isSafeInteger(q.x)||!Number.isSafeInteger(q.y))throw Error('Array coordinates are out of range.');return q;};
    const angle=(value:number)=>transform.rotation?normalized(value+transform.rotation):value;
    for(const original of source){
      let item=structuredClone(original);
      if('locked'in item)item.locked=false;
      switch(item.kind){
        case 'footprint':item={...item,pos:point(item.pos),rotation:angle(item.rotation),designator:designator(item.designator),padNets:keepNets?item.padNets:Object.fromEntries(Object.keys(item.padNets).map(key=>[key,'']))};break;
        case 'line':case 'track':item={...item,a:point(item.a),b:point(item.b)};break;
        case 'dimension':item={...item,a:point(item.a),b:point(item.b),angle:angle(item.angle)};break;
        case 'rect':item=transform.rotation?{kind:'polyline',points:[item.a,{x:item.b.x,y:item.a.y},item.b,{x:item.a.x,y:item.b.y}].map(point),closed:true,width:item.width,layer:item.layer,fill:item.fill}:{...item,a:point(item.a),b:point(item.b)};break;
        case 'arc':case 'arcTrack':item={...item,center:point(item.center),startAngle:angle(item.startAngle)};break;
        case 'circle':item={...item,center:point(item.center)};break;
        case 'polyline':item={...item,points:item.points.map(point)};break;
        case 'zone':item={...item,outline:item.outline.map(point),fill:[]};break;
        case 'text':item={...item,pos:point(item.pos),rotation:angle(item.rotation)};break;
        case 'via':item={...item,pos:point(item.pos)};break;
      }
      if(!keepNets&&'net'in item)item.net='';out.push(item);
    }
  }
  return out;
}
/** Pure plan; the origin cell is retained and all added cells share one undo command. */
export function rectangularArray(items:ReadonlyMap<ItemId,Item>,ids:Iterable<ItemId>,options:ArrayOptions):Item[]{
  const {columns,rows,dx,dy,keepNets}=options;
  if(!Number.isSafeInteger(columns)||!Number.isSafeInteger(rows)||columns<1||rows<1||columns>100||rows>100||columns*rows<2)throw Error('Use 1–100 rows and columns, with at least two cells.');
  if(!Number.isSafeInteger(dx)||!Number.isSafeInteger(dy)||(columns>1&&dx===0)||(rows>1&&dy===0))throw Error('Enter non-zero spacing for each repeated direction.');
  const source=sources(items,ids),transforms:Transform[]=[];
  for(let row=0;row<rows;row++)for(let col=0;col<columns;col++)if(row||col)transforms.push({point:p=>({x:p.x+col*dx,y:p.y+row*dy}),rotation:0});
  return copies(items,source,transforms,keepNets);
}
/** Positive angles go counterclockwise; copies may rotate or retain their original orientation. */
export function circularArray(items:ReadonlyMap<ItemId,Item>,ids:Iterable<ItemId>,options:CircularArrayOptions):Item[]{
  const {count,center,angle,rotate,keepNets}=options;
  if(!Number.isInteger(count)||count<2||count>100)throw Error('Use 2–100 circular positions, including the original.');
  if(!Number.isSafeInteger(center.x)||!Number.isSafeInteger(center.y)||!Number.isFinite(angle)||Math.abs(angle)>360)throw Error('Enter a valid center and an angle between -360° and 360°.');
  const source=sources(items,ids),anchor=arrayAnchor(items,ids),seen=new Set([0]),transforms:Transform[]=[];
  if(anchor.x===center.x&&anchor.y===center.y)throw Error('Choose a center different from the selection reference point.');
  for(let i=1;i<count;i++){
    const rotation=normalized(i*angle),key=Math.round(rotation*1e8)%36000000000;
    if(seen.has(key))throw Error('The angle repeats an existing position. Reduce the count or change the angle.');seen.add(key);
    const target=rotatePoint(anchor,center,rotation),dx=Math.round(target.x)-anchor.x,dy=Math.round(target.y)-anchor.y;
    transforms.push({point:p=>{const q=rotate?rotatePoint(p,center,rotation):{x:p.x+dx,y:p.y+dy};return{x:Math.round(q.x),y:Math.round(q.y)};},rotation:rotate?rotation:0});
  }
  return copies(items,source,transforms,keepNets);
}

export interface SimilarOptions { layer:boolean; net:boolean; size:boolean; value:boolean; footprint:boolean }
/** Available criteria for a seed; unsupported fields are never silently compared. */
export function similarCriteria(item:Item):(keyof SimilarOptions)[]{
  const keys:(keyof SimilarOptions)[]=[];
  if('layer' in item||item.kind==='footprint'||item.kind==='via')keys.push('layer');
  if('net' in item)keys.push('net');
  if(['track','arcTrack','via','pad','text','line','arc','circle','rect','polyline'].includes(item.kind))keys.push('size');
  if(item.kind==='footprint')keys.push('value','footprint');
  return keys;
}
function property(item:Item,key:keyof SimilarOptions):unknown{
  if(key==='layer')return item.kind==='footprint'?item.side:item.kind==='via'?[Math.min(item.fromLayer,item.toLayer),Math.max(item.fromLayer,item.toLayer)]:('layer' in item?item.layer:null);
  if(key==='net')return 'net' in item?item.net:null;
  if(key==='value')return item.kind==='footprint'?item.value:null;
  if(key==='footprint')return item.kind==='footprint'?item.footprint:null;
  if(item.kind==='via')return [item.diameter,item.drill];
  if(item.kind==='pad')return [item.width,item.height,item.shape,item.padKind,item.drill];
  if(item.kind==='text')return item.height;
  if(item.kind==='rect')return [Math.abs(item.b.x-item.a.x),Math.abs(item.b.y-item.a.y),item.width];
  if(item.kind==='circle'||item.kind==='arc')return [item.radius,item.width];
  return 'width' in item?item.width:null;
}
export function similarItems(items:ReadonlyMap<ItemId,Item>,seed:Item,options:SimilarOptions,allowed:(item:Item)=>boolean=()=>true):ItemId[]{
  const criteria=similarCriteria(seed).filter(key=>options[key]);
  const expected=criteria.map(key=>JSON.stringify(property(seed,key)));
  const out:ItemId[]=[];
  for(const [id,item]of items)if(item.kind===seed.kind&&allowed(item)&&criteria.every((key,i)=>JSON.stringify(property(item,key))===expected[i]))out.push(id);
  return out;
}
