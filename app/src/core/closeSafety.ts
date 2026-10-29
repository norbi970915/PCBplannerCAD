/** Failure to inspect unsaved work must never be mistaken for a clean window. */
export async function allowWindowClose(options:{
  unsaved:()=>Promise<string[]>;
  confirm:(names:string[])=>Promise<boolean>;
  capture:()=>Promise<unknown>;
  report:(message:string)=>void;
}):Promise<boolean>{
  let names:string[];try{names=await options.unsaved();}catch(e){options.report('Cannot check unsaved work; the window was kept open. '+String(e));return false;}
  if(!names.length)return true;
  let quit:boolean;try{quit=await options.confirm(names);}catch(e){options.report('Cannot confirm closing; the window was kept open. '+String(e));return false;}
  if(!quit)return false;
  try{await options.capture();}catch(e){options.report('Final recovery copy failed. '+String(e));}
  return true;
}
