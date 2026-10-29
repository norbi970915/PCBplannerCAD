import {api} from './api';
import {bus} from './bus';
import {useEditor} from './store';
import type {DocState} from './store';
import type {Item,ItemId,ProjectInfo} from './types';
export function linkedTargets(project:ProjectInfo|null,source:DocState):string[]{
  if(!project)return [];
  const sheets=project.sheets.flatMap(s=>s.docId?[s.docId]:[]);
  if(source.info.kind==='schematic'&&sheets.includes(source.info.id))return project.pcb?.docId?[project.pcb.docId]:[];
  if(source.info.kind==='pcb'&&source.info.id===project.pcb?.docId)return sheets;
  return [];
}
export function designator(item:Item|undefined):string{
  return item?.kind==='part'?item.fields.find(f=>f.name==='Designator')?.value.trim()??'':item?.kind==='footprint'?item.designator.trim():'';
}
let request=0;
/** Navigation only: the native document and its Undo history remain unchanged. */
export async function revealLinkedComponent(sourceId:string,itemId:ItemId):Promise<boolean>{
  const state=useEditor.getState(),source=state.docs[sourceId],part=source?.items.get(itemId),project=state.project,token=++request;
  const warn=(message:string)=>{useEditor.getState().setHint(message);useEditor.getState().log('warning',message,{source:'Linked component'});return false;};
  if(!source||part?.kind!=='part'||!project||!project.sheets.some(s=>s.docId===sourceId))return false;
  const ref=designator(part),target=project.pcb?.docId;
  if(!ref||ref.includes('?'))return warn('Annotate this component before locating it on the board.');
  if(!target)return warn('The linked board is unavailable. Open or repair the project board first.');
  const current=()=>token===request&&useEditor.getState().project===project&&useEditor.getState().docs[sourceId]?.items.get(itemId)===part;
  try{
    if(!useEditor.getState().docs[target]){
      const snapshot=await api.snapshot(target);if(!current())return false;
      useEditor.getState().loadDoc(snapshot);
    }
    if(!current())return false;
    const board=useEditor.getState().docs[target];if(board?.info.kind!=='pcb')return false;
    const matches=[...board.items].filter(([,item])=>item.kind==='footprint'&&designator(item)===ref);
    if(matches.length!==1)return warn(matches.length?'More than one board component uses '+ref+'; resolve the duplicate designator first.':ref+' has no placed footprint on the linked board.');
    const [id,footprint]=matches[0];if(footprint.kind!=='footprint')return false;
    const side=footprint.side==='bottom'?1:0;
    useEditor.getState().setView(target,{pads:true,hiddenLayers:board.view.hiddenLayers.filter(l=>l!==side&&l!==15)});
    useEditor.getState().patchDoc(target,{tool:'select',activeLayer:side,selection:new Set([id]),marked:new Set([id]),focus:{pos:footprint.pos,seq:Date.now(),ids:[id]}});
    bus.emit('showDoc',{docId:target});useEditor.getState().setActiveDoc(target);
    useEditor.getState().setHint('Located '+ref+' on the linked board.');return true;
  }catch(e){if(current())return warn('Cannot show the linked component: '+String(e));return false;}
}
