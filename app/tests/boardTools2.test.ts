import test from 'node:test';
import assert from 'node:assert/strict';
import { netRows, packFootprints } from '../src/pcb/boardTools.ts';
import { findMatches, matcher } from '../src/core/find.ts';
import { dimensionLabel, dimensionPrims, dimensionThrough } from '../src/editor/dimension.ts';
import type { DimensionItem, Footprint, Item } from '../src/core/types.ts';

const mm = (v: number) => Math.round(v * 1e6);
const p = (x: number, y: number) => ({ x: mm(x), y: mm(y) });
const pad = (number: string, x: number) => ({ kind: 'pad', number, pos: p(x, 0), width: mm(1), height: mm(1), rotation: 0, shape: 'rect', padKind: 'smd', layer: 0, drill: null, cornerRatio: 0 }) as Item;
const footprints: Record<string, Footprint> = {
  R: { name: 'R', description: '', items: [pad('1', -1), pad('2', 1)], model: null }, // 3 × 1 mm
  U: { name: 'U', description: '', items: [pad('1', -4), pad('2', 4), { ...pad('3', 0), pos: p(0, 3) } as Item], model: null }, // 9 × 4 mm
};
const part = (designator: string, footprint: string, x: number, y: number, value = '', nets: Record<string, string> = {}): Item =>
  ({ kind: 'footprint', designator, value, footprint, pos: p(x, y), rotation: 0, side: 'top', locked: false, padNets: nets, component: '', parameters: {} }) as Item;

test('find ranks exact designators first and supports wildcards and nets', () => {
  const items = new Map<number, Item>([
    [1, part('R1', 'R', 0, 0, '10k', { '1': 'GND' })],
    [2, part('R12', 'R', 5, 0, '1k')],
    [3, part('U1', 'U', 20, 0, 'R1-buffer')],
    [4, { kind: 'track', a: p(0, 0), b: p(1, 0), width: mm(0.2), layer: 0, net: 'GND' } as Item],
  ]);
  const r1 = findMatches(items, 'r1');
  assert.deepEqual(r1.map((m) => m.label), ['R1', 'R12', 'U1'], 'exact, then prefix, then a value match');
  assert.deepEqual(findMatches(items, 'R?').map((m) => m.label), ['R1']);
  const gnd = findMatches(items, 'gnd');
  assert.equal(gnd[0].kind, 'net');
  assert.deepEqual(gnd[0].ids, [1, 4]);
  assert.equal(matcher('  '), null);
  assert.deepEqual(findMatches(items, '10K').map((m) => m.label), ['R1'], 'values match case-insensitively');
});

test('pack & move puts footprints in tight rows from the top-left, tallest first, on whole grid steps', () => {
  const items = new Map<number, Item>([[1, part('R1', 'R', 50, 0)], [2, part('U1', 'U', 0, 20)], [3, part('R2', 'R', 30, -10)]]);
  const moved = new Map(packFootprints(items, footprints, [1, 2, 3], mm(1), mm(0.5)).map((r) => [r.id, (r.item as { pos: { x: number; y: number } }).pos]));
  // Selection box: left −4.5 (U1), top 23.5 (U1's top pad). Row width ≈ 9.75 mm (√ of the gapped areas × 1.2):
  // U1 (tallest) fills the first row, R1 and R2 share the second, 1 mm below it.
  assert.equal(moved.get(2), undefined, 'U1 already sits at the top-left');
  assert.deepEqual(moved.get(1), p(-3, 18));
  assert.deepEqual(moved.get(3), p(1, 18), 'R2 right of R1 with a 1 mm gap');
});

test('nets panel rows split routed length per layer', () => {
  const items = new Map<number, Item>([
    [1, { kind: 'track', a: p(0, 0), b: p(3, 4), width: mm(0.2), layer: 0, net: 'A' } as Item],
    [2, { kind: 'track', a: p(0, 0), b: p(0, 2), width: mm(0.2), layer: 1, net: 'A' } as Item],
    [3, { kind: 'via', pos: p(0, 0), diameter: mm(0.6), drill: mm(0.3), fromLayer: 0, toLayer: 1, net: 'A' } as Item],
  ]);
  const [row] = netRows(items, []).rows;
  assert.equal(row.length, mm(7));
  assert.deepEqual([...row.byLayer], [[0, mm(5)], [1, mm(2)]]);
  assert.equal(row.vias, 1);
});

const base = { kind: 'dimension', layer: 4, width: mm(0.1), textHeight: mm(1), units: 'mm', precision: 2 } as const;

test('dimension geometry matches the core: line at the offset, value beside it, readable text', () => {
  const d: DimensionItem = { ...base, a: p(0, 0), b: p(20, 3), offset: mm(5), orthogonal: true, angle: 0 };
  const prims = dimensionPrims(d);
  assert.equal(prims.length, 8);
  const line = prims[2] as { a: unknown; b: unknown };
  assert.deepEqual([line.a, line.b], [p(0, 5), p(20, 5)]);
  const text = prims[7] as { text: string; pos: unknown; rotation: number };
  assert.deepEqual([text.text, text.pos, text.rotation], ['20.00 mm', p(10, 5.8), 0]);
  assert.equal(dimensionLabel({ ...d, orthogonal: false, b: p(30, 40) }), '50.00 mm');
  assert.equal(dimensionLabel({ ...d, orthogonal: true, angle: 90, b: p(30, 40), units: 'mil', precision: 0 }), '1575 mil');
});

test('the third click sets the dimension line through the cursor for every style', () => {
  const v = dimensionThrough(p(0, 0), p(10, 4), p(3, -6), 'horizontal', base);
  assert.equal(v.offset, mm(-6));
  assert.equal(dimensionLabel(v), '10.00 mm');
  const vert = dimensionThrough(p(0, 0), p(10, 4), p(14, 1), 'vertical', base);
  assert.equal(vert.offset, mm(-14), 'left normal of +Y is −X');
  assert.equal(dimensionLabel(vert), '4.00 mm');
  const al = dimensionThrough(p(0, 0), p(10, 0), p(5, 2), 'aligned', base);
  assert.equal(al.offset, mm(2));
});


test('rectangular arrays preserve the original cell, allocate unique references and explicitly handle nets', async()=>{
  const {rectangularArray}=await import('../src/pcb/selectionTools.ts');
  const original=part('R1','R',2,3,'10k',{'1':'A','2':'GND'});
  if(original.kind!=='footprint')throw Error('fixture');original.locked=true;
  const items=new Map<number,Item>([[1,original],[2,part('r2','R',0,0)],[3,part('R4','R',0,0)]]);
  const copies=rectangularArray(items,[1],{columns:2,rows:2,dx:mm(5),dy:mm(-4),keepNets:false});
  assert.equal(copies.length,3);
  assert.deepEqual(copies.map(i=>i.kind==='footprint'?i.designator:''),['R3','R5','R6']);
  assert.deepEqual(copies.map(i=>'pos'in i?i.pos:null),[p(7,3),p(2,-1),p(7,-1)]);
  assert.equal(original.locked,true);assert.equal(original.padNets['1'],'A');
  assert.ok(copies.every(i=>i.kind==='footprint'&&!i.locked&&Object.values(i.padNets).every(n=>n==='')));
  const kept=rectangularArray(items,[1],{columns:2,rows:1,dx:mm(5),dy:0,keepNets:true})[0];
  assert.equal(kept.kind==='footprint'&&kept.padNets['1'],'A');
  if(kept.kind==='footprint')kept.padNets['1']='B';
  assert.equal(original.padNets['1'],'A','nested maps are independent');
});

test('array plans translate copper and discard copied pour fills without mutating source geometry',async()=>{
  const {rectangularArray}=await import('../src/pcb/selectionTools.ts');
  const track={kind:'track',a:p(1,2),b:p(3,4),width:mm(.2),layer:0,net:'A',locked:true}as Item;
  const zone={kind:'zone',net:'A',outline:[p(0,0),p(2,0),p(0,2)],fill:[{outer:[p(0,0),p(2,0),p(0,2)],holes:[]}],teardrop:false}as Item;
  const out=rectangularArray(new Map([[1,track],[2,zone]]),[1,2],{columns:2,rows:1,dx:mm(10),dy:0,keepNets:false});
  assert.ok(out[0].kind==='track');if(out[0].kind==='track'){assert.deepEqual(out[0].a,p(11,2));assert.equal(out[0].net,'');assert.equal(out[0].locked,false);}
  assert.ok(out[1].kind==='zone');if(out[1].kind==='zone'){assert.deepEqual(out[1].outline[0],p(10,0));assert.deepEqual(out[1].fill,[]);}
  assert.deepEqual(track.kind==='track'&&track.a,p(1,2));
});

test('arrays reject unsupported selections, overlapping cells, excessive size and unsafe coordinates',async()=>{
  const {rectangularArray}=await import('../src/pcb/selectionTools.ts');
  const items=new Map<number,Item>([[1,part('R1','R',0,0)]]);
  const options={columns:2,rows:1,dx:mm(5),dy:0,keepNets:true};
  assert.throws(()=>rectangularArray(items,[999],options),/Select board objects/);
  assert.throws(()=>rectangularArray(items,[1],{...options,dx:0}),/spacing/);
  assert.throws(()=>rectangularArray(items,[1],{...options,columns:1}),/at least two/);
  assert.throws(()=>rectangularArray(items,[1],{...options,dx:NaN}),/spacing/);
  assert.throws(()=>rectangularArray(items,[1],{...options,columns:3,dx:Number.MAX_SAFE_INTEGER}),/coordinates/);
  assert.throws(()=>rectangularArray(new Map([[1,items.get(1)!],[2,part('R2','R',0,0)]]),[1,2],{...options,columns:100,rows:100,dy:1}),/10,000/);
});

test('similar-object selection combines explicit size/net/layer criteria and respects allowed objects',async()=>{
  const {similarItems}=await import('../src/pcb/selectionTools.ts');
  const seed={kind:'track',a:p(0,0),b:p(1,0),width:mm(.2),layer:0,net:'A'}as Item;
  const items=new Map<number,Item>([[1,seed],[2,{...seed,net:'B'}as Item],[3,{...seed,layer:1}as Item],[4,{...seed,width:mm(.3)}as Item],[5,part('R1','R',0,0)]]);
  const options={layer:true,net:false,size:true,value:false,footprint:false};
  assert.deepEqual(similarItems(items,seed,options),[1,2]);
  assert.deepEqual(similarItems(items,seed,{...options,net:true}),[1]);
  assert.deepEqual(similarItems(items,seed,{...options,layer:false}),[1,2,3]);
  assert.deepEqual(similarItems(items,seed,options,item=>!('net'in item)||item.net!=='A'),[2]);
});


test('Find applies its object-type filter before the result limit',()=>{
  const items=new Map<number,Item>();for(let i=1;i<=250;i++)items.set(i,part('R'+i,'R',0,0,'GND'));
  items.set(999,{kind:'track',a:p(0,0),b:p(1,0),width:100,layer:0,net:'GND'}as Item);
  assert.equal(findMatches(items,'gnd',1,'net')[0].kind,'net');
  assert.deepEqual(findMatches(items,'gnd',1,'text'),[]);
});

test('custom grid steps remain precise and adjacent presets never skip or reverse direction',async()=>{
  const {adjacentGrid,formatGrid,parseLength,validGrid}=await import('../src/core/units.ts');
  assert.equal(adjacentGrid(mm(.125),[mm(.1),mm(.25),mm(.5)],1),mm(.25));
  assert.equal(adjacentGrid(mm(.125),[mm(.1),mm(.25),mm(.5)],-1),mm(.1));
  assert.equal(adjacentGrid(mm(20),[mm(.1),mm(.5)],1),mm(20));
  assert.equal(formatGrid(1,'mm'),'0.000001mm');assert.equal(parseLength(formatGrid(127000,'mil'),'mm'),127000);
  assert.equal(validGrid(0),false);assert.equal(validGrid(Infinity),false);assert.equal(validGrid(1),true);
});

test('measurement can record, reverse endpoints and return to editing without creating geometry',async()=>{
  const {measureTool}=await import('../src/editor/tools/measure.ts');const {useEditor}=await import('../src/core/store.ts');
  const tool=measureTool();let cursor=p(1,2),edits=0;
  const env={docId:'board',cursor:()=>cursor,redraw:()=>{},exec:async()=>{edits++;return true;}}as unknown as import('../src/editor/tools/types.ts').ToolEnv;
  tool.down!(env,{}as never);cursor=p(4,6);tool.down!(env,{}as never);
  assert.equal(tool.key!(env,'Enter',{}as never),true);
  const message=useEditor.getState().messages.at(-1)!;assert.equal(message.source,'measure');assert.match(message.text,/5.000mm/);
  tool.key!(env,'Tab',{}as never);tool.key!(env,'Enter',{}as never);assert.deepEqual(useEditor.getState().messages.at(-1)!.target!.pos,p(4,6));
  tool.key!(env,'Backspace',{}as never);assert.match(tool.hint(env),/second point/);assert.equal(edits,0);
});


test('board component browsing uses natural reference order and filters side, lock and footprint/value',async()=>{
  const {componentRows}=await import('../src/pcb/componentBrowser.ts');
  const a=part('R10','Resistors/R',0,0,'10k'),b={...part('R2','Resistors/R',0,0,'1k'),side:'bottom',locked:true}as Item;
  const items=new Map<number,Item>([[1,a],[2,b],[3,part('C1','Capacitors/C',0,0,'100n')]]);
  const all={query:'',side:'all' as const,lock:'all' as const};
  assert.deepEqual(componentRows(items,all).map(row=>row.item.designator),['C1','R2','R10']);
  assert.deepEqual(componentRows(items,{...all,query:'resistors'}).map(row=>row.id),[2,1]);
  assert.deepEqual(componentRows(items,{...all,side:'bottom',lock:'locked'}).map(row=>row.id),[2]);
  assert.deepEqual(componentRows(items,{...all,query:'10K',lock:'unlocked'}).map(row=>row.id),[1]);
});


test('circular arrays rotate positions and orientations or preserve orientation as requested',async()=>{
  const {circularArray}=await import('../src/pcb/selectionTools.ts');
  const items=new Map<number,Item>([[1,part('U1','R',10,0,'A',{'1':'N'})]]);
  const opts={count:4,center:p(0,0),angle:90,rotate:true,keepNets:true};
  const rotated=circularArray(items,[1],opts);
  assert.deepEqual(rotated.map(i=>'pos'in i?i.pos:null),[p(0,10),p(-10,0),p(0,-10)]);
  assert.deepEqual(rotated.map(i=>i.kind==='footprint'?i.rotation:null),[90,180,270]);
  assert.deepEqual(circularArray(items,[1],{...opts,rotate:false}).map(i=>i.kind==='footprint'?i.rotation:null),[0,0,0]);
  assert.throws(()=>circularArray(items,[1],{...opts,count:5}),/repeats/);
  assert.throws(()=>circularArray(items,[1],{...opts,center:p(10,0)}),/different/);
});

test('rotating a rectangular graphic in an array preserves its four corners as a polygon',async()=>{
  const {circularArray}=await import('../src/pcb/selectionTools.ts');
  const rect={kind:'rect',a:p(9,-1),b:p(11,1),width:100,layer:2,fill:'none'}as Item;
  const [copy]=circularArray(new Map([[1,rect]]),[1],{count:2,center:p(0,0),angle:45,rotate:true,keepNets:false});
  assert.equal(copy.kind,'polyline');if(copy.kind==='polyline'){assert.equal(copy.points.length,4);assert.equal(copy.closed,true);assert.equal(copy.width,100);assert.equal(copy.layer,2);}
  assert.equal(rect.kind,'rect','source geometry is unchanged');
});


test('message logs can be filtered by source/severity/text and exported with timestamps',async()=>{
  const {filterMessages,messageLogText}=await import('../src/core/messageLog.ts');
  const time=new Date('2026-10-27T10:00:00Z');
  const messages=[{id:1,time,level:'error',text:'Clearance problem',source:'drc'},{id:2,time,level:'info',text:'5mm',source:'measure'}]as import('../src/core/store.ts').Message[];
  const filtered=filterMessages(messages,{level:'error',source:'drc',query:'CLEARANCE'});
  assert.equal(filtered.length,1);assert.equal(filterMessages(messages,{level:'all',source:'',query:'measure'})[0].id,2);
  assert.equal(messageLogText(filtered),'2026-10-27T10:00:00.000Z [ERROR] [drc] Clearance problem');
});


test('net sorting prioritizes unrouted work or length with stable natural-name ties',async()=>{
  const {sortNetRows}=await import('../src/pcb/boardTools.ts');
  const rows=[{name:'N10',unrouted:2,length:100},{name:'N2',unrouted:2,length:300},{name:'N1',unrouted:0,length:200}]as import('../src/pcb/boardTools.ts').NetRow[];
  assert.deepEqual(sortNetRows(rows,'unrouted',true).map(r=>r.name),['N2','N10','N1']);
  assert.deepEqual(sortNetRows(rows,'length',true).map(r=>r.name),['N2','N1','N10']);
  assert.deepEqual(rows.map(r=>r.name),['N10','N2','N1'],'sorting does not mutate cached data');
});
