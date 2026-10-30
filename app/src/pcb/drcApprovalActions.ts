import {api} from '../core/api';
import {report} from '../core/actions';
import {useEditor} from '../core/store';
import type {DrcApproval,DrcViolation} from '../core/types';
import {findingKey,makeDrcApproval} from './drcApprovals';

const pending=new Set<string>();
/** Native updates merge into the latest board settings under its document lock. */
export async function setDrcApproval(docId:string,finding:DrcViolation,approve:boolean,stored?:DrcApproval):Promise<boolean>{
  if(pending.has(docId))return false;
  const doc=useEditor.getState().docs[docId];if(!doc||doc.info.kind!=='pcb')return false;
  if(approve&&(doc.drcStatus!=='current'||!doc.drc.some(v=>findingKey(v)===findingKey(finding)))){
    report('Run DRC again before approving this finding.');return false;
  }
  pending.add(docId);
  try{
    const approval=stored??makeDrcApproval(doc,finding);
    const update=await api.pcbDrcApproval(docId,approval,approve);
    useEditor.getState().applyUpdate(update);return true;
  }catch(error){report(error);return false;}finally{pending.delete(docId);}
}
