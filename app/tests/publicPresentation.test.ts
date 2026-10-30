import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync, existsSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { fileURLToPath } from 'node:url';

// Hashes keep the prohibited names themselves out of repository prose and identifiers.
const prohibited = new Set([
  "0cd12e4e1f9d9bbb494a20d02001dbcb126bd926b2530b2f4c2173510de3238c",
  "eae42e054dccebedd2a659cb294d2aeb8bde85aa518eafe769caf623888883aa",
  "db56b9c3e2c79f93d78cae6dc74cee748d44324c1a39c9064af8e53785e8cd3e",
  "e73b48f750be953532c1d1757b5ba081e8a6b0408ea8b4e65ef1e65631a7da06",
  "f5d36c673cea1d80af3d33404d506e0c3e7aeaa0b8c1a21e085fa3caa8928b3b",
  "19ee8ad8b95df6cdae7cf74cbca6fa98c3e10b866bd4363a62db1b107d83fda6",
  "e9efcdc81fe315f202a4ec0a67be5c563868af2a2896451d8ded6cd185011b6f",
  "aa300fb004a5d73d7fa7da3f2452867d4698693e7deb0486f11f72755d8e8ea9",
  "9cd881aba255ed5aa74ba0df3dd7b5db87d983c36ec78e2961a0ec51b8096463",
  "2a7c3fdf03fe5ea63f57f986a5c5b4e93d2fe13958a1b98db45a2c2bf58df6a8",
  "0ad84a2678a1d82516c5c8569fc8fcf7a2f438e810efe7438f80bf0afca5751c",
  "4f6b9e1c24d93d9aeb4607b9d21de1f9345b735ec1b786dd663b5e9fe5171091",
  "d9d771df57e86d113220244726d7c795611a3583075927d292a4b159b24a5e4d",
  "320d597682ed208cdaa58f77d17df6037e29f3c48323d6d16b23e0ca18f97788",
  "063de0773639dd9f2b3047465a825bea1c752c53177f51acd573cf1e17accdbc",
  "4ec90cb5a1d1b69299b71a63c3453c4c1870629a9b11d189b40b805bb67d54ad"
]);
const structuredPrefix=Buffer.from('6b69636164','hex').toString();
const xmlRoot=Buffer.from('6561676c65','hex').toString();
const digest=(word:string)=>createHash('sha256').update(word.toLowerCase()).digest('hex');
const root=fileURLToPath(new URL('../../',import.meta.url));

function hasProhibitedName(text:string):boolean {
  const variants=[text,text.replace(/([a-z0-9])([A-Z])/g,'$1 $2')];
  for(const variant of variants)for(const token of variant.match(/[a-z][a-z0-9]*/gi)??[]){
    for(let size=3;size<=Math.min(16,token.length);size++)if(prohibited.has(digest(token.slice(0,size))))return true;
  }
  return false;
}

function withoutWireMarkers(file:string,text:string):string {
  // Exact external extensions and root tags remain required for existing files.
  text=text.replace(new RegExp('\\b'+structuredPrefix+'_(?:symbol_lib|pcb|pro|sch|sym|mod|wks|dru)\\b','g'),'wire_format');
  if(file.startsWith('crates/import/src/xml/')){
    text=text.replace(new RegExp('(<\\/?)'+xmlRoot+'(?=[\\s/>])','g'),'$1wire_root')
      .replace(new RegExp('"'+xmlRoot+'"','g'),'"wire_root"')
      .replace(new RegExp('(<!DOCTYPE\\s+)'+xmlRoot+'(?=\\s)','g'),'$1wire_root')
      .replaceAll(xmlRoot+'.dtd','wire.dtd');
  }
  if(file==='crates/output/src/assembly.rs')text=text.replace('#[serde(alias = "'+structuredPrefix+'")]','#[serde(alias = "legacy_format")]').replace('r#""'+structuredPrefix+'""#','r#""legacy_format""#');
  return text;
}

test('repository naming uses neutral paths, identifiers and prose; only exact wire markers are exempt',()=>{
  const files=execFileSync('git',['ls-files','--cached','--others','--exclude-standard','-z'],{cwd:root,encoding:'utf8'}).split('\0').filter(Boolean);
  for(const file of files){
    if(!existsSync(new URL(file,new URL('../../',import.meta.url))))continue;
    if(/^(?:LICENSE|NOTICE|COPYING)(?:[./-]|$)/i.test(file))continue;
    assert(!hasProhibitedName(file),'Naming policy violation in path: '+file);
    if(!/\.(?:rs|ts|tsx|mjs|md|toml|json|html|yml|yaml|cmd)$/.test(file))continue;
    const text=withoutWireMarkers(file,readFileSync(new URL(file,new URL('../../',import.meta.url)),'utf8'));
    for(const[line,value]of text.split(/\r?\n/).entries())assert(!hasProhibitedName(value),'Naming policy violation at '+file+':'+(line+1));
  }
});

test('naming checks reject prose and internal identifier variants without blocking ordinary words',()=>{
  const name=structuredPrefix;
  assert(hasProhibitedName(name));assert(hasProhibitedName(name+'Paths'));assert(hasProhibitedName('import_'+name+'_project'));
  assert(!hasProhibitedName('PCBPlanner Studio copper pads and format parsing; confusion accuracy'));
  assert(!hasProhibitedName(withoutWireMarkers('crates/import/src/structured/board.rs','"'+name+'_pcb"')));
  assert(!hasProhibitedName(withoutWireMarkers('crates/import/src/structured/board.rs','"'+name+'_dru"')));
  assert(hasProhibitedName(withoutWireMarkers('app/src/App.tsx','"'+xmlRoot+'"')));
});
