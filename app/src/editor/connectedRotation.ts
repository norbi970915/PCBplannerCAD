import { api } from '../core/api';
import { useEditor, violatorModeOf, type DocState } from '../core/store';
import type { Command, ItemId, Point } from '../core/types';
import { moveRejection } from './connectedMove';

const requests=new Map<string,number>();
/** Idle rotation and menu rotation use the same checked copper plan as a connected drag. */
export async function rotateWithConnections(docId:string,ids:ItemId[],center:Point,angle:number,exec:(cmd:Command)=>Promise<boolean>,getDoc:()=>DocState|undefined,stillCurrent:()=>boolean=()=>true):Promise<boolean>{
  const doc=getDoc();if(!doc)return false;
  const ticket=(requests.get(docId)??0)+1;requests.set(docId,ticket);
  const state=useEditor.getState(),pl=state.placement,routeMode=violatorModeOf(state);
  const eligible=doc.info.kind==='pcb'&&pl.moveMode!=='disconnect'&&ids.some(id=>{const item=doc.items.get(id);return item?.kind==='footprint'?Object.values(item.padNets).some(Boolean):item?.kind==='via'&&!!item.net;});
  if(!eligible)return exec({op:'rotate',ids,center,angle});
  const selection=[...doc.selection].sort((a,b)=>a-b).join(',');
  const plan=await api.pcbDragFootprints(docId,{ids,delta:{x:0,y:0},center,angle,flipped:false,mode:pl.moveMode,routeMode,corner:pl.cornerStyle});
  const current=getDoc(),settings=useEditor.getState();
  if(requests.get(docId)!==ticket||!stillCurrent())return false;
  if(!current||current.items!==doc.items||current.footprints!==doc.footprints||current.epoch!==doc.epoch||[...current.selection].sort((a,b)=>a-b).join(',')!==selection)throw Error('Board or selection changed while planning rotation; try again.');
  if(settings.placement.moveMode!==pl.moveMode||violatorModeOf(settings)!==routeMode||settings.placement.cornerStyle!==pl.cornerStyle)throw Error('Move settings changed while planning rotation; try again.');
  const rejection=moveRejection(plan,routeMode);if(rejection)throw Error(rejection);
  if(!plan.replace.length&&!plan.delete.length&&!plan.add.length)return false;
  return exec({op:'route',delete:plan.delete,add:plan.add,replace:plan.replace});
}
