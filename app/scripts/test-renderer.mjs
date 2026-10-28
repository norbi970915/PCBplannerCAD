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
const server=await createServer({configFile:false,root,resolve:{alias:{'@':path.join(root,'src')}},optimizeDeps:{noDiscovery:true},plugins:[{name:'local-render-fixture',configureServer(server){server.middlewares.use('/__fixture',(_req,res)=>{res.setHeader('Content-Type','application/json');res.end(fixture??'null');});}}],server:{host:'127.0.0.1',port:0,strictPort:false},logLevel:'error'});
let browser;
try{
 await server.listen();const port=server.httpServer.address().port;
 browser=await chromium.launch({headless:true,...(process.env.PCAD_BROWSER_EXECUTABLE?{executablePath:process.env.PCAD_BROWSER_EXECUTABLE}:process.platform==='win32'?{channel:'msedge'}:{})});
 const page=await browser.newPage({viewport:{width:640,height:480}}),errors=[];
 page.on('pageerror',e=>errors.push(String(e)));
 await page.goto(`http://127.0.0.1:${port}/tests/renderer-harness.html${fixture?'?fixture=1':''}`);
 await page.waitForFunction(()=>typeof window.runRendererChecks==='function');
 const result=await page.evaluate(()=>window.runRendererChecks());
 if(errors.length)throw Error(errors.join('\n'));
 console.log(JSON.stringify(result,null,2));
}finally{await browser?.close();await server.close();}