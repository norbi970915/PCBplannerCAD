const LIMIT = 8192;
function summarize(values: number[], count: number) {
  const sorted=values.slice().sort((a,b)=>a-b);
  const at=(q:number)=>sorted.length?+sorted[Math.min(sorted.length-1,Math.floor(sorted.length*q))].toFixed(3):0;
  return {count,samples:sorted.length,medianMs:at(.5),p95Ms:at(.95),maxMs:at(1)};
}
/** Opt-in local CPU diagnostics; no geometry, filenames or automatic transmission. */
export class CanvasTimings {
  active=false;
  private began=0;
  private frames:number[]=[];
  private scenes:number[]=[];
  private frameCount=0;
  private sceneCount=0;
  constructor(private now:()=>number=()=>performance.now()){}
  start(){this.frames=[];this.scenes=[];this.frameCount=0;this.sceneCount=0;this.began=this.now();this.active=true;}
  frame(ms:number){if(this.active&&Number.isFinite(ms)&&ms>=0)this.frames[this.frameCount++%LIMIT]=ms;}
  scene<T>(fn:()=>T):T{
    if(!this.active)return fn();const t=this.now();
    try{return fn();}finally{this.scenes[this.sceneCount++%LIMIT]=Math.max(0,this.now()-t);}
  }
  stop(){
    if(!this.active)return null;this.active=false;
    return {elapsedMs:Math.max(0,this.now()-this.began),frameCpu:summarize(this.frames,this.frameCount),sceneCpu:summarize(this.scenes,this.sceneCount),note:'CPU timings only. Scene work may be included in a frame. GPU completion and presented FPS are not measured.'};
  }
}