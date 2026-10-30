import {api} from '../core/api';
import type {BoardItem} from '../core/types';
export interface RoutingDefaults{width:number;pairWidth:number;pairGap:number}
const cache=new WeakMap<BoardItem,Map<string,{value?:RoutingDefaults;pending:Promise<RoutingDefaults>}>>();
const key=(net:string,layer:number)=>JSON.stringify([net,layer]);
export function cachedRoutingDefaults(board:BoardItem|undefined,net:string,layer:number):RoutingDefaults|undefined{return board?cache.get(board)?.get(key(net,layer))?.value:undefined;}
/** Cache by the immutable settings item; edits and Undo naturally invalidate it. */
export function loadRoutingDefaults(board:BoardItem,net:string,layer:number):Promise<RoutingDefaults>{
 let entries=cache.get(board);if(!entries){entries=new Map();cache.set(board,entries);}const id=key(net,layer),existing=entries.get(id);if(existing)return existing.pending;
 const entry:{value?:RoutingDefaults;pending:Promise<RoutingDefaults>}={pending:Promise.resolve(null as unknown as RoutingDefaults)};
 entry.pending=api.pcbRoutingDefaults(board,net,layer).then(value=>{entry.value=value;return value;}).catch(error=>{entries!.delete(id);throw error;});entries.set(id,entry);return entry.pending;
}
