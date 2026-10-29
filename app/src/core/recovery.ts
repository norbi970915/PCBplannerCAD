import {create} from 'zustand';
import {api,type RecoveryInfo} from './api';
import {RecoveryTimer} from './recoveryTimer';
import {useEditor} from './store';
const KEY='pcbplannercad.recovery.enabled';
function enabled(){try{return localStorage.getItem(KEY)!=='0';}catch{return true;}}
export const useRecoveryStatus=create<{enabled:boolean;hasCopies:boolean;last:RecoveryInfo|null;error:string;setEnabled:(value:boolean)=>void}>(set=>({
  enabled:enabled(),hasCopies:false,last:null,error:'',
  setEnabled:value=>{try{localStorage.setItem(KEY,value?'1':'0');}catch{/* Optional preference. */}set({enabled:value});}
}));
export function installRecoveryAutosave(){
  let live=true;
  void api.recoveryList().then(list=>{if(live)useRecoveryStatus.setState({hasCopies:!!list.entries.length});}).catch(e=>{if(live)useRecoveryStatus.setState({error:String(e)});});
  const timer=new RecoveryTimer({
    enabled:()=>useRecoveryStatus.getState().enabled,
    capture:()=>api.recoveryCapture(false),
    success:result=>{const info=result as RecoveryInfo|null;useRecoveryStatus.setState({error:'',...(info?{last:info,hasCopies:true}:{})});},
    failure:e=>{const error=String(e);if(error!==useRecoveryStatus.getState().error)useEditor.getState().log('warning','Recovery copy failed: '+error,{source:'Recovery'});useRecoveryStatus.setState({error});}
  });
  const first=window.setTimeout(()=>void timer.tick(),10000),interval=window.setInterval(()=>void timer.tick(),60000);
  return()=>{live=false;timer.dispose();window.clearTimeout(first);window.clearInterval(interval);};
}
