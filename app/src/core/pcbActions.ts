import {approvalOnlyChange,reviewDrc} from "../pcb/drcApprovals";
import {linkedTargets,designator} from "./crossProbe";
import { toast } from "sonner";
import { api } from "./api";
import { report } from "./actions";
import { bus } from "./bus";
import { useEditor } from "./store";
import type { DrcViolation, ItemId, Point } from "./types";
import { NM_PER_MM } from "./units";
import { mergeViolations, ONLINE_SKIP, touchedBetween, type Touched } from "../pcb/drcMerge";

const st = () => useEditor.getState();
const timers = new Map<string, number>();
const analysisVersions = new Map<string,number>();

/** Refreshes the ratsnest (and online DRC) shortly after board edits. */
export function scheduleAnalysis(docId: string, delay = 200) {
  const version=(analysisVersions.get(docId)??0)+1;analysisVersions.set(docId,version);
  window.clearTimeout(timers.get(docId));
  timers.set(
    docId,
    window.setTimeout(async () => {
      const snapshot=st().docs[docId];if(!snapshot)return;
      try {
        const a = await api.pcbAnalysis(docId);
        const current=st().docs[docId];
        if(version!==analysisVersions.get(docId)||!current||current.items!==snapshot.items||current.epoch!==snapshot.epoch)return;
        st().patchDoc(docId, { airwires: a.airwires });
        const unrouted = a.stats.unrouted;
        if(st().activeDocId===docId&&current.tool==="select")st().setHint(unrouted ? `${unrouted} of ${a.stats.connections} connections unrouted` : st().hint);
      } catch {
        // Analysis is advisory; ignore transient errors (e.g. doc closed).
      }
    }, delay),
  );
}

const drcVersions = new Map<string, number>();
const manualDrc = new Set<string>();
function beginDrc(docId:string) {
  const doc=st().docs[docId];if(!doc)return null;
  const version=(drcVersions.get(docId)??0)+1;drcVersions.set(docId,version);
  st().patchDoc(docId,{drcStatus:'checking',drcError:null});
  return {items:doc.items,footprints:doc.footprints,epoch:doc.epoch,version};
}
function sameDrcBoard(docId:string,token:NonNullable<ReturnType<typeof beginDrc>>) {
  const doc=st().docs[docId];return !!doc&&doc.items===token.items&&doc.footprints===token.footprints&&doc.epoch===token.epoch;
}
export async function runDrc(docId = st().activeDocId) {
  if (!docId || manualDrc.has(docId) || st().docs[docId]?.info.kind !== "pcb") return;
  st().setActiveDoc(docId);
  st().patchDoc(docId,{inspectorTab:"drc",drcRunId:(st().docs[docId].drcRunId??0)+1});
  bus.emit("showPanel",{id:"properties",docId});
  const token=beginDrc(docId);if(!token)return;
  manualDrc.add(docId);let failed=false;
  try {
    const r = await api.pcbDrc(docId);
    if(drcVersions.get(docId)!==token.version)return;
    if(!sameDrcBoard(docId,token)) { if(st().onlineDrc)onlineJob(docId).full=true;st().patchDoc(docId,{drcStatus:'stale'});st().log('info','The board changed during DRC. Run the check again for current results.');return; }
    st().patchDoc(docId, { drc:r.violations, drcScope:'full', drcStatus:'current', drcCheckedAt:Date.now(), drcRuleErrors:r.ruleErrors, drcError:null });
    st().clearMessages("drc");
    for (const e of r.ruleErrors) st().log("warning", 'Rule: '+e, { source:"drc" });
    const reviewed=reviewDrc(st().docs[docId]);
    for (const v of reviewed.open) st().log(v.severity, v.message, { source:"drc", target:{ docId,items:v.items,pos:v.pos } });
    const errors=reviewed.open.filter(v=>v.severity==='error').length;
    st().log(errors?'error':'info', 'DRC: '+errors+' errors, '+(reviewed.open.length-errors)+' warnings · '+reviewed.current.size+' approved · '+r.stats.unrouted+' of '+r.stats.connections+' connections unrouted.', {source:'drc'});
    // The document-specific DRC inspector remains selected; messages retain the log.
  } catch(e) {
    failed=true;
    if(st().onlineDrc)onlineJob(docId).full=true;
    if(drcVersions.get(docId)===token.version)st().patchDoc(docId,{drcStatus:'error',drcError:String(e)});
    report(e);
  } finally {
    manualDrc.delete(docId);const job=online.get(docId);
    if(!failed&&st().onlineDrc&&job&&(job.full||job.touched.drop.size))scheduleOnline(docId,0);
  }
}

export async function repour(docId = st().activeDocId, zone: ItemId | null = null, quiet = false) {
  if (!docId) return;
  try {
    st().applyUpdate(await api.pcbRepour(docId, zone));
    scheduleAnalysis(docId, 0);
    if (!quiet) st().log("info", zone === null ? "Repoured all copper pours." : "Repoured the selected pour.");
  } catch (e) {
    report(e);
  }
}

export async function refreshFootprints(docId: string) {
  st().patchDoc(docId, { footprints: await api.pcbFootprints(docId) });
}

export async function newProjectBoard() {
  try {
    const p = await api.projectNewPcb();
    st().setProject(p.project);
    const id = p.project?.pcb?.docId;
    if (id) {
      st().loadDoc(await api.snapshot(id));
      bus.emit("showDoc", { docId: id });
      scheduleAnalysis(id, 0);
    }
  } catch (e) {
    report(e);
  }
}

/** Opens the project board, creating it on first use. */
export async function openProjectBoard() {
  const p = st().project;
  if (!p) return report("Open or create a project first.");
  const id = p.pcb?.docId;
  if (!id) return newProjectBoard();
  if (!st().docs[id]) {
    const t = toast.loading("Opening board…");
    try {
      const start = performance.now();
      const snap = await api.snapshot(id);
      const loaded = performance.now();
      st().loadDoc(snap);
      bus.emit("showDoc", { docId: id });
      // Two frames: the canvas has built its scene and drawn once.
      await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
      const shown = performance.now();
      const s = (ms: number) => `${(ms / 1000).toFixed(2)} s`;
      st().log("info", `Board opened: ${snap.items.length} objects in ${s(shown - start)} (backend snapshot ${s(loaded - start)}, drawing ${s(shown - loaded)}).`);
    } catch (e) {
      return report(e);
    } finally {
      toast.dismiss(t);
    }
  }
  bus.emit("showDoc", { docId: id });
  scheduleAnalysis(id, 0);
}

export async function highlightNetOnBoard(docId: string, net: string) {
  try {
    st().patchDoc(docId, { marked: new Set(await api.pcbNetItems(docId, net)) });
  } catch (e) {
    report(e);
  }
}

/** Net of the copper under `pos`: the active layer first, then the other copper layers of the stack. */
export async function netAt(docId: string, pos: Point): Promise<string | null> {
  const d = st().docs[docId];
  if (!d) return null;
  const board = [...d.items.values()].find((i) => i.kind === "board");
  const cu = board?.kind === "board" ? board.stackup.layers.filter((l) => l.kind === "copper" && l.layer !== null).map((l) => l.layer as number) : [0, 1];
  const at = { x: Math.round(pos.x), y: Math.round(pos.y) };
  for (const layer of [d.activeLayer, ...cu.filter((l) => l !== d.activeLayer)].filter((l) => cu.includes(l))) {
    const hit = await api.pcbObjectAt(docId, at, layer);
    if (hit?.[1]) return hit[1];
  }
  return null;
}

/** Alt+click / `: highlights the whole net under the cursor; on open board it clears the highlight. */
export async function highlightNetAt(docId: string, pos: Point) {
  try {
    const net = await netAt(docId, pos);
    if (!net) {
      st().patchDoc(docId, { marked: new Set() });
      st().setHint("No net under the cursor · highlight cleared");
      return;
    }
    const [ids, length] = await Promise.all([api.pcbNetItems(docId, net), api.pcbNetLength(docId, net)]);
    st().patchDoc(docId, { marked: new Set(ids) });
    st().setHint(`Net ${net}: ${ids.length} objects · routed length ${(length / NM_PER_MM).toFixed(2)} mm · Esc clears`);
  } catch (e) {
    report(e);
  }
}

/** I: selects the tracks and vias of the net under the cursor. */
export async function selectNetCopperAt(docId: string, pos: Point) {
  try {
    const net = await netAt(docId, pos);
    if (!net) return;
    const items = st().docs[docId]?.items;
    const ids = (await api.pcbNetItems(docId, net)).filter((id) => ["track", "arcTrack", "via"].includes(items?.get(id)?.kind ?? ""));
    st().patchDoc(docId, { selection: new Set(ids) });
    st().setHint(`Net ${net}: ${ids.length} tracks and vias selected`);
  } catch (e) {
    report(e);
  }
}

// ---------- online DRC ----------

interface OnlineJob {
  full: boolean;
  touched: Touched;
  timer: number;
  running: boolean;
}
const online = new Map<string, OnlineJob>();

function onlineJob(docId: string): OnlineJob {
  let j = online.get(docId);
  if (!j) online.set(docId, (j = { full: false, touched: { check: new Set(), drop: new Set() }, timer: 0, running: false }));
  return j;
}

function scheduleOnline(docId: string, delay = 250) {
  const j = onlineJob(docId);
  window.clearTimeout(j.timer);
  st().patchDoc(docId,{drcStatus:'checking',drcError:null});
  j.timer = window.setTimeout(() => void runOnline(docId), delay);
}

/** One request at a time per board; stale results never replace newer geometry. */
async function runOnline(docId: string) {
  const j=onlineJob(docId);
  if(j.running||manualDrc.has(docId))return;
  if(!st().onlineDrc||st().docs[docId]?.info.kind!=='pcb'){online.delete(docId);return;}
  const full=j.full,touched=j.touched;
  if(!full&&!touched.drop.size){if(st().docs[docId]?.drcStatus==='checking')st().patchDoc(docId,{drcStatus:st().docs[docId].drcCheckedAt?'current':'unchecked'});return;}
  j.full=false;j.touched={check:new Set(),drop:new Set()};j.running=true;
  const token=beginDrc(docId);if(!token){j.running=false;return;}
  let failed=false;
  try {
    let violations:DrcViolation[],ruleErrors=st().docs[docId]?.drcRuleErrors??[];
    if(full){const r=await api.pcbDrc(docId);violations=r.violations.filter(v=>!ONLINE_SKIP.has(v.code));ruleErrors=r.ruleErrors;}
    else {
      const ids=[...touched.check].filter(id=>token.items.has(id));
      const fresh=ids.length?await api.pcbDrcNear(docId,ids):[];
      violations=mergeViolations(st().docs[docId]?.drc??[],touched.drop,fresh);
    }
    if(!st().onlineDrc||drcVersions.get(docId)!==token.version)return;
    if(!sameDrcBoard(docId,token)) {
      // Requeue the in-flight batch too: the next near-check must cover both sets of edits.
      j.full ||= full || st().docs[docId]?.epoch!==token.epoch;
      for(const id of touched.check)j.touched.check.add(id);
      for(const id of touched.drop)j.touched.drop.add(id);
      return;
    }
    st().patchDoc(docId,{drc:violations,drcScope:'online',drcStatus:'current',drcCheckedAt:Date.now(),drcRuleErrors:ruleErrors,drcError:null});
  }catch(e){
    failed=true;j.full=true;
    if(st().onlineDrc&&drcVersions.get(docId)===token.version)st().patchDoc(docId,{drcStatus:'error',drcError:String(e)});
  }finally{
    j.running=false;
    if(!failed&&online.get(docId)===j&&st().onlineDrc&&st().docs[docId]&&(j.full||j.touched.drop.size))scheduleOnline(docId,0);
  }
}

/** Errors and warnings currently marked on a board (what online DRC keeps up to date). */
export function drcCounts(docId: string): { errors: number; warnings: number } {
  const v = st().docs[docId]?.drc ?? [];
  const errors = v.filter((x) => x.severity === "error").length;
  return { errors, warnings: v.length - errors };
}

/**
 * Online DRC: after every board change (routing, moves, dialogs, undo / redo) the changed objects are
 * re-checked with the near-DRC and their markers replaced. Opening a board or switching it on runs one full check.
 */
export function installOnlineDrc(): () => void {
  const stop=()=>{
    for(const [id,j]of online){window.clearTimeout(j.timer);if(!manualDrc.has(id)){drcVersions.set(id,(drcVersions.get(id)??0)+1);if(st().docs[id]?.drcStatus==='checking')st().patchDoc(id,{drcStatus:'stale'});}}
    online.clear();
  };
  const baseline=()=>{for(const[id,d]of Object.entries(st().docs))if(d.info.kind==='pcb'){onlineJob(id).full=true;scheduleOnline(id,0);}};
  if(st().onlineDrc)baseline();
  const unsubscribe=useEditor.subscribe((s,prev)=>{
    if(!s.onlineDrc){if(prev.onlineDrc)stop();return;}
    if(!prev.onlineDrc){baseline();return;}
    if(s.docs===prev.docs)return;
    for(const[id,d]of Object.entries(s.docs)){
      if(d.info.kind!=='pcb')continue;
      const old=prev.docs[id];
      if(old?.items===d.items&&old?.epoch===d.epoch&&old?.footprints===d.footprints)continue;
      if(old&&old.epoch===d.epoch&&old.footprints===d.footprints&&approvalOnlyChange(old.items,d.items))continue;
      const j=onlineJob(id);
      let rulesChanged=false;
      if(old){for(const[key,item]of d.items)if(item.kind==='board'&&old.items.get(key)!==item){rulesChanged=true;break;}
        if(!rulesChanged)for(const[key,item]of old.items)if(item.kind==='board'&&!d.items.has(key)){rulesChanged=true;break;}}
      if(!old||old.epoch!==d.epoch||old.footprints!==d.footprints||rulesChanged)j.full=true;
      else touchedBetween(old.items,d.items,j.touched);
      if(j.full||j.touched.drop.size)scheduleOnline(id);
      else if(d.drcStatus==='stale'&&old?.drcStatus==='current')st().patchDoc(id,{drcStatus:'current'});
    }
    for(const[id,j]of online)if(!s.docs[id]){window.clearTimeout(j.timer);online.delete(id);drcVersions.set(id,(drcVersions.get(id)??0)+1);}
  });
  return()=>{unsubscribe();stop();};
}

// ---------- cross-probing ----------

/** Selecting parts in a schematic marks their footprints on the board and vice versa. */
export function installCrossProbe(): () => void {
  let busy = false;
  return useEditor.subscribe((s, prev) => {
    if (busy || s.activeDocId === null) return;
    const doc = s.docs[s.activeDocId];
    const before = prev.docs[s.activeDocId];
    if (!doc || doc.selection === before?.selection) return;
    const designators = new Set<string>();
    for (const id of doc.selection) {
      const it = doc.items.get(id);
      if (it?.kind === "part") {
                  const d = designator(it);
        if (d) designators.add(d);
      } else if (it?.kind === "footprint") designators.add(designator(it));
    }
    const targetKind = doc.info.kind === "schematic" ? "pcb" : doc.info.kind === "pcb" ? "schematic" : null;
    if (!targetKind) return;
    busy = true;
    try {
      for (const id of linkedTargets(s.project,doc)) {
        const other=s.docs[id];if (!other||other.info.kind !== targetKind) continue;
        const marked = new Set<ItemId>();
        if (designators.size) {
          for (const [id, it] of other.items) {
            if (it.kind === "footprint" && designators.has(designator(it))) marked.add(id);
            if (it.kind === "part" && designators.has(designator(it))) marked.add(id);
          }
        }
        if (marked.size || other.marked.size) st().patchDoc(other.info.id, { marked });
      }
    } finally {
      busy = false;
    }
  });
}
