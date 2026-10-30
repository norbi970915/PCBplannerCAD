// Collect complete notices and versioned covered sources locally. Never fetch during a build.
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { createRequire } from 'node:module';
import { copyFileSync, existsSync, mkdirSync, readdirSync, readFileSync, realpathSync, statSync, writeFileSync } from 'node:fs';
import { basename, dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
const root=resolve(dirname(fileURLToPath(import.meta.url)),'..');
const output=join(root,'LICENSE/generated');mkdirSync(join(output,'sources'),{recursive:true});
const hash=bytes=>createHash('sha256').update(bytes).digest('hex');
const noticePattern=/^(licen[cs]e|notice|copying|copyright)([._-]|$)/i;
const records=[],seen=new Set();
const declared=text=>typeof text==='string'?text:JSON.stringify(text);
function readNotices(dir,name,version){
  const files=readdirSync(dir).filter(file=>noticePattern.test(file)&&statSync(join(dir,file)).isFile());
  if(files.length)return files.map(file=>({file,text:readFileSync(join(dir,file),'utf8').trim()}));
  const fallback=join(root,'LICENSE/upstream',`${name.replaceAll('/','__')}@${version}.txt`);
  if(!existsSync(fallback))throw Error(`Missing full notice for ${name}@${version}; review and add a version-specific upstream notice.`);
  return [{file:'reviewed upstream notice',text:readFileSync(fallback,'utf8').trim()}];
}
function visitJs(manifest){
  const pkg=JSON.parse(readFileSync(manifest,'utf8')),key=pkg.name+'@'+pkg.version;
  if(seen.has(key))return;seen.add(key);const dir=dirname(manifest);
  if(pkg.name!=='pcbplannercad-app')records.push({ecosystem:'npm',name:pkg.name,version:pkg.version,license:declared(pkg.license),repository:pkg.repository,notices:readNotices(dir,pkg.name,pkg.version)});
  for(const name of Object.keys(pkg.dependencies??{})){
    let candidate=join(dir,'node_modules',name,'package.json');
    if(!existsSync(candidate)){
      const req=createRequire(manifest);let d=dirname(req.resolve(name));
      while(d!==dirname(d)&&!existsSync(join(d,'package.json')))d=dirname(d);
      candidate=join(d,'package.json');if(JSON.parse(readFileSync(candidate,'utf8')).name!==name)throw Error('Unresolved dependency '+name);
    }
    visitJs(realpathSync(candidate));
  }
}
visitJs(join(root,'app/package.json'));
if(records.some(record=>record.name==='occt-import-js'))throw Error('STEP preview dependency is excluded from this release.');
const cargo=process.platform==='win32'?'cargo.exe':'cargo';
const metadata=JSON.parse(execFileSync(cargo,['metadata','--offline','--locked','--format-version','1','--filter-platform','x86_64-pc-windows-msvc'],{cwd:root,encoding:'utf8',maxBuffer:30*1024*1024}));
for(const pkg of metadata.packages.filter(p=>p.source)){
  const dir=dirname(pkg.manifest_path);
  const notices=readNotices(dir,pkg.name,pkg.version);
  if(pkg.license_file){const file=join(dir,pkg.license_file);if(existsSync(file)&&!notices.some(n=>n.file===pkg.license_file))notices.push({file:pkg.license_file,text:readFileSync(file,'utf8').trim()});}
  const record={ecosystem:'cargo',name:pkg.name,version:pkg.version,license:pkg.license,repository:pkg.repository,notices};
  if(/MPL-2\.0/.test(pkg.license??'')){
    const archive=`${pkg.name}-${pkg.version}.crate`;
    const cached=resolve(dir,'../../../cache',basename(dirname(dir)),archive);
    if(!existsSync(cached))throw Error('Required covered-source archive is unavailable: '+archive);
    const target=join(output,'sources',archive);copyFileSync(cached,target);
    record.coveredSource={file:'sources/'+archive,sha256:hash(readFileSync(target)),license:'MPL-2.0'};
  }
  records.push(record);
}
records.push({ecosystem:'adapted-source',name:'shadcn/ui',version:'2023 copyright notice',license:'MIT',repository:'https://github.com/shadcn-ui/ui',notices:[{file:'LICENSE.md',text:readFileSync(join(root,'LICENSE/upstream/shadcn-ui.txt'),'utf8').trim()}]});
records.push({ecosystem:'adapted-source',name:'Tauri installer templates',version:'2.12.4; explicit shortcut icon modifications',license:'MIT',repository:'https://github.com/tauri-apps/tauri/tree/tauri-cli-v2.12.4/crates/tauri-bundler/src/bundle/windows',notices:[{file:'LICENSE-MIT',text:readFileSync(join(root,'LICENSE/upstream/tauri-installer-templates@2.12.4.txt'),'utf8').trim()}]});
records.sort((a,b)=>(a.ecosystem+'/'+a.name).localeCompare(b.ecosystem+'/'+b.name));
const intro='PCBPlanner Studio — third-party notices\n\nThese components retain their own licenses. This inventory includes resolved build tools as well as application dependencies; it does not claim every package is incorporated into the installed executable. The covered-source archives accompany this application. Original PCBPlanner Studio material is separately licensed.\n';
const body=intro+records.map(record=>`\n${'-'.repeat(72)}\n${record.ecosystem}: ${record.name} ${record.version}\nDeclared license: ${record.license}\nUpstream: ${typeof record.repository==='string'?record.repository:record.repository?.url??'see package metadata'}\n${record.coveredSource?'Covered source: '+record.coveredSource.file+' (SHA-256 '+record.coveredSource.sha256+')\n':''}\n`+record.notices.map(n=>n.text).join('\n\n')).join('\n');
writeFileSync(join(output,'third-party-notices.txt'),body);
const manifest={schemaVersion:1,target:'x86_64-pc-windows-msvc',packageLockSha256:hash(readFileSync(join(root,'pnpm-lock.yaml'))),cargoLockSha256:hash(readFileSync(join(root,'Cargo.lock'))),records:records.map(({notices,...record})=>({...record,noticeSha256:hash(notices.map(n=>n.text).join('\n\n'))}))};
writeFileSync(join(output,'manifest.json'),JSON.stringify(manifest,null,2)+'\n');
copyFileSync(join(root,'LICENSE.md'),join(output,'PCBPlanner-Studio-free-use-terms.md'));
console.log(`Prepared ${records.length} dependency/adapted-source notices and ${records.filter(r=>r.coveredSource).length} MPL source archives.`);
