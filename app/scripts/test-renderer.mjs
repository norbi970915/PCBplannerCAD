// Isolated headless browser: never attaches to an existing user browser/window.
import fs from 'node:fs';
import path from 'node:path';
import {fileURLToPath,pathToFileURL} from 'node:url';
import {createServer} from 'vite';
const root=fileURLToPath(new URL('../',import.meta.url));
const packagePath=process.env.PCAD_PLAYWRIGHT_PATH;
let chromium;
try{({chromium}=await import(packagePath?pathToFileURL(path.join(packagePath,'index.mjs')).href:'playwright'));}
catch{throw Error('Install Playwright for this check or set PCAD_PLAYWRIGHT_PATH to its package directory.');}
const fixture=process.argv[2]?fs.readFileSync(path.resolve(process.argv[2]),'utf8'):null;
const checkBuiltWorker=process.env.PCAD_TEST_BUILT_WORKER==='1';
const assetDirectory=path.join(root,'dist/assets');
const builtWorker=checkBuiltWorker&&fs.existsSync(assetDirectory)?fs.readdirSync(assetDirectory).find(name=>/^pourTriangles\.worker-.+\.js$/.test(name)):null;
if(checkBuiltWorker&&!builtWorker)throw Error('Build the frontend before checking its worker asset.');
const applicationCsp=JSON.parse(fs.readFileSync(path.join(root,'src-tauri/tauri.conf.json'),'utf8')).app.security.csp;
const productionWorkerScript=`
 const worker=new Worker(${JSON.stringify('/dist/assets/'+builtWorker)},{type:'module'});
 worker.onmessage=({data})=>{
  if(data.error)throw Error(data.error);
  const tris=new Float64Array(data.triangles);let area=0;
  for(let i=0;i<tris.length;i+=6)area+=Math.abs((tris[i+2]-tris[i])*(tris[i+5]-tris[i+1])-(tris[i+4]-tris[i])*(tris[i+3]-tris[i+1]))/2;
  if(Math.abs(area/1e12-96)>1e-6)throw Error('Built fill worker changed polygon area');
  window.builtWorkerResult={status:'passed',areaMm2:area/1e12,triangles:tris.length/6};worker.terminate();
 };
 worker.onerror=event=>{throw Error(event.message||'Built worker failed');};
 const p=(x,y)=>({x:x*1e6,y:y*1e6});
 worker.postMessage({id:1,polygon:{outer:[p(0,0),p(10,0),p(10,10),p(0,10)],holes:[[p(2,2),p(4,2),p(4,4),p(2,4)]]}});
`;
const server=await createServer({configFile:false,root,resolve:{alias:{'@':path.join(root,'src')}},optimizeDeps:{noDiscovery:true},plugins:[{name:'local-render-fixture',configureServer(server){
 server.middlewares.use('/__built_worker_check',(_req,res)=>{res.setHeader('Content-Type','text/html');res.setHeader('Content-Security-Policy',applicationCsp);res.end('<!doctype html><script src="/__built_worker_entry.js"></script>');});
 server.middlewares.use('/__built_worker_entry.js',(_req,res)=>{res.setHeader('Content-Type','text/javascript');res.end(productionWorkerScript);});
 server.middlewares.use('/__fixture',(_req,res)=>{res.setHeader('Content-Type','application/json');res.end(fixture??'null');});}}],server:{host:'127.0.0.1',port:0,strictPort:false},logLevel:'error'});
let browser;
try{
 await server.listen();const port=server.httpServer.address().port;
 browser=await chromium.launch({headless:true,...(process.env.PCAD_BROWSER_EXECUTABLE?{executablePath:process.env.PCAD_BROWSER_EXECUTABLE}:process.platform==='win32'?{channel:'msedge'}:{})});
 const page=await browser.newPage({viewport:{width:640,height:480}}),errors=[];
 page.on('pageerror',e=>errors.push(String(e)));
 const query=new URLSearchParams();if(fixture)query.set('fixture','1');if(process.env.PCAD_SYNC_FILLS==='1')query.set('syncFills','1');
 await page.goto(`http://127.0.0.1:${port}/tests/renderer-harness.html?${query}`);
 await page.waitForFunction(()=>typeof window.runRendererChecks==='function');
 const result=await page.evaluate(()=>window.runRendererChecks());
 if(checkBuiltWorker){await page.goto(`http://127.0.0.1:${port}/__built_worker_check`);await page.waitForFunction(()=>window.builtWorkerResult?.status==='passed');result.builtWorker=await page.evaluate(()=>window.builtWorkerResult);}
 if(errors.length)throw Error(errors.join('\n'));
 console.log(JSON.stringify(result,null,2));
}finally{await browser?.close();await server.close();}