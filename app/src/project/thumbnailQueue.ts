export interface Thumbnail {url:string|null;description:string;error?:string}
type Job={key:string;generation:number;load:(alive:()=>boolean)=>Promise<Thumbnail|null>;listeners:Set<(value:Thumbnail)=>void>};
/** One visible-file job at a time, bounded raster cache, no document mutations. */
export class ThumbnailQueue{
  private jobs=new Map<string,Job>();private queue:Job[]=[];private active=false;private cache=new Map<string,Thumbnail>();generation=0;
  constructor(private limit=96){}
  clear(){this.generation++;this.cache.clear();this.queue=[];this.jobs.clear();}
  request(key:string,load:Job['load'],deliver:(value:Thumbnail)=>void){
    const cached=this.cache.get(key);if(cached){this.cache.delete(key);this.cache.set(key,cached);let live=true;const generation=this.generation;queueMicrotask(()=>{if(live&&generation===this.generation)deliver(cached);});return()=>{live=false;};}
    let job=this.jobs.get(key);if(!job){job={key,generation:this.generation,load,listeners:new Set()};this.jobs.set(key,job);this.queue.push(job);}job.listeners.add(deliver);void this.pump();
    return()=>{job!.listeners.delete(deliver);};
  }
  private async pump(){if(this.active)return;const job=this.queue.shift();if(!job)return;
    const alive=()=>job.generation===this.generation&&job.listeners.size>0;
    if(!alive()){this.jobs.delete(job.key);void this.pump();return;}this.active=true;
    try{await new Promise(resolve=>setTimeout(resolve,0));if(!alive())return;let result:Thumbnail|null;
      try{result=await job.load(alive);}catch(error){result={url:null,description:'Preview unavailable; open the file to inspect it',error:String(error)};}
      if(alive()&&result){this.cache.set(job.key,result);while(this.cache.size>this.limit)this.cache.delete(this.cache.keys().next().value!);for(const callback of job.listeners)callback(result);}
    }finally{if(this.jobs.get(job.key)===job)this.jobs.delete(job.key);this.active=false;void this.pump();}
  }
}
