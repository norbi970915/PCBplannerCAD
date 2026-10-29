export interface RecoveryTimerOptions {
  enabled:()=>boolean;
  capture:()=>Promise<unknown>;
  success:(result:unknown)=>void;
  failure:(error:unknown)=>void;
}
/** Timer callbacks never overlap; disposal ignores late completions. */
export class RecoveryTimer {
  private running=false;
  private disposed=false;
  constructor(private options:RecoveryTimerOptions){}
  async tick(){
    if(this.running||this.disposed||!this.options.enabled())return;
    this.running=true;
    try{const result=await this.options.capture();if(!this.disposed)this.options.success(result);}
    catch(e){if(!this.disposed)this.options.failure(e);}
    finally{this.running=false;}
  }
  dispose(){this.disposed=true;}
}
