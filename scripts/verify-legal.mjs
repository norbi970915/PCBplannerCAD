import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { existsSync, readFileSync, readdirSync, statSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
const root=resolve(dirname(fileURLToPath(import.meta.url)),'..');
const read=path=>readFileSync(join(root,path),'utf8');
const config=JSON.parse(read('app/src-tauri/tauri.conf.json'));
assert.equal(config.identifier,'com.pcbplanner.studio');assert.equal(config.bundle.publisher,'Buchmüller Norbert');
assert.equal(config.productName,'PCBPlanner Studio');assert.equal(config.mainBinaryName,'PCBPlannerStudio');assert(config.bundle.licenseFile);assert(config.bundle.resources['../../LICENSE/']==='legal/');
const manifest=JSON.parse(read('LICENSE/generated/manifest.json'));
const hash=bytes=>createHash('sha256').update(bytes).digest('hex');
assert.equal(manifest.packageLockSha256,hash(readFileSync(join(root,'pnpm-lock.yaml'))));
assert.equal(manifest.cargoLockSha256,hash(readFileSync(join(root,'Cargo.lock'))));
assert(!manifest.records.some(r=>r.name==='occt-import-js'));
for(const record of manifest.records.filter(r=>r.coveredSource))assert.equal(hash(readFileSync(join(root,'LICENSE/generated',record.coveredSource.file))),record.coveredSource.sha256);
for(const path of ['LICENSE.md','LICENSE/PRIVACY.md','LICENSE/SOURCE-AVAILABILITY.md','LICENSE/generated/third-party-notices.txt'])assert(read(path).length>200,path);
assert(!read('app/src/pcb/dialogs.tsx').includes('±10% typical'));
assert(!read('app/src/library/FootprintWizard.tsx').includes('IPC-7351B nominal'));
const dist=join(root,'app/dist');
if(process.argv.includes('--dist')){
  assert(existsSync(dist),'Build the frontend first');
  const walk=dir=>readdirSync(dir).flatMap(name=>{const file=join(dir,name);return statSync(file).isDirectory()?walk(file):[file]});
  const files=walk(dist);assert(!files.some(file=>/\.wasm$|occt|step-bindings/i.test(file)),'STEP preview engine must not be emitted');
  const text=files.filter(f=>/\.(js|html)$/.test(f)).map(f=>readFileSync(f,'utf8')).join('\n');
  assert(text.includes('Third-party licenses')&&text.includes('Buchmüller Norbert'),'Legal UI is missing');
  assert(!text.includes('eu.allengra.pcbplannercad'),'Legacy identity must not appear in frontend UI');
}
console.log('Personal publisher, license files, source hashes, release claims and excluded preview engine verified.');
