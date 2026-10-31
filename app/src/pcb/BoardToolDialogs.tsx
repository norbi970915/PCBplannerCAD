import { useEffect, useMemo, useRef, useState } from "react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { NativeSelect, NativeSelectOption } from "@/components/ui/native-select";
import { LengthInput } from "@/components/app/fields";
import { api } from "../core/api";
import { execute, report } from "../core/actions";
import { layer } from "../core/layers";
import { scheduleAnalysis } from "../core/pcbActions";
import { useEditor } from "../core/store";
import type { CleanupOptions, CleanupResult, Item, ItemId, TeardropOptions, ViaItem } from "../core/types";
import { formatLength, NM_PER_MM } from "../core/units";
import { Dialog, useModal } from "../shell/modal";
import { boardStats } from "./boardTools";
import { cleanupSource, cleanupReviewCurrent, type CleanupReview } from "./cleanupReview";
import { VIA_KIND_LABEL, type ViaKind } from "./vias";

const close = () => useModal.getState().close();

function Check({ checked, onChange, label, count, disabled }: { checked: boolean; onChange: (v: boolean) => void; label: string; count?: number; disabled?: boolean }) {
  return (
    <label className="flex items-center gap-2 py-0.5">
      <Checkbox className="size-3.5" disabled={disabled} checked={checked} onCheckedChange={(v) => onChange(v === true)} />
      <span className="flex-1">{label}</span>
      {count !== undefined && <span className={count ? "font-mono text-ink" : "font-mono text-faint"}>{count}</span>}
    </label>
  );
}

// ---------- Cleanup Tracks & Vias ----------

const CLEANUP_LABELS: [keyof CleanupOptions, string, keyof CleanupResult][] = [
  ["mergeCollinear", "Merge collinear segments", "merged"],
  ["duplicates", "Delete redundant (overlapping) segments", "duplicates"],
  ["zeroLength", "Delete zero-length segments", "zeroLength"],
  ["dangling", "Remove open stubs and trim free tails", "dangling"],
  ["unconnectedVias", "Delete vias connected on one layer or none", "vias"],
];

export function CleanupDialog({ docId }: { docId: string }) {
  const doc = useEditor((s) => s.docs[docId]);
  const [opts, setOpts] = useState<CleanupOptions>({ mergeCollinear: true, duplicates: true, zeroLength: true, dangling: true, unconnectedVias: false });
  const [review, setReview] = useState<CleanupReview | null>(null);
  const [applying,setApplying]=useState(false);
  const [error,setError]=useState<string|null>(null);
  const [refresh,setRefresh]=useState(0);
  const mounted=useRef(false);const currentDocId=useRef(docId);currentDocId.current=docId;
  useEffect(()=>{mounted.current=true;return()=>{mounted.current=false;};},[]);
  useEffect(()=>setError(null),[docId,opts,doc?.items,doc?.footprints,doc?.epoch]);
  useEffect(() => {
    let live=true;setReview(null);
    const source=cleanupSource(docId,opts,doc);
    if(source) api.pcbCleanup(docId,opts).then(
      plan=>{if(live)setReview({source,plan});},
      e=>{if(live){setError(String(e));report(e);}}
    );
    return()=>{live=false;};
  },[docId,opts,doc?.items,doc?.footprints,doc?.epoch,refresh]);
  const plan=cleanupReviewCurrent(review,docId,opts,doc)?review!.plan:null;
  const changes=plan?plan.delete.length+plan.replace.length:0;
  const run=async()=>{
    if(applying)return;
    setApplying(true);setError(null);
    try{
      const state=useEditor.getState();
      if(state.activeDocId!==docId||!cleanupReviewCurrent(review,docId,opts,state.docs[docId])||!plan)
        throw new Error("Board changed; wait for the current cleanup preview");
      const update=await api.pcbCleanupApply(docId,opts,plan);
      useEditor.getState().applyUpdate(update);
      scheduleAnalysis(docId,0);
      toast.success("Cleanup: "+plan.delete.length+" removed, "+plan.trimmed+" trimmed, "+plan.merged+" merged");
      if(mounted.current&&currentDocId.current===docId)close();
    }catch(e){
      report(e);
      if(mounted.current&&currentDocId.current===docId){setError(String(e));setReview(null);setRefresh(n=>n+1);}
    }finally{if(mounted.current&&currentDocId.current===docId)setApplying(false);}
  };
  return (
    <Dialog title="Cleanup tracks & vias"
      description="Tidies routing while preserving useful connections and locked copper. Counts update as you pick options; one undo step."
      footer={<>
        <Button variant="outline" disabled={applying} onClick={close}>Cancel</Button>
        <Button disabled={applying||!changes} onClick={()=>void run()}>{applying?"Applying…":plan?"Apply ("+changes+")":"Checking…"}</Button>
      </>}>
      <div className="grid">
        {CLEANUP_LABELS.map(([k,label,count])=>(
          <Check key={k} label={label} disabled={applying} checked={opts[k]} onChange={v=>setOpts({...opts,[k]:v})}
            count={opts[k]&&plan?k==="dangling"?plan.dangling+plan.trimmed:plan[count] as number:undefined}/>
        ))}
        {plan&&plan.trimmed>0&&<p className="mt-1 text-[11px] text-muted-foreground">{plan.trimmed} useful tracks will have free tails trimmed.</p>}
        {error&&<p role="alert" className="mt-2 text-[11px] text-danger">{error}</p>}
        <p className="mt-2 text-[11px] text-muted-foreground">Open stub chains are removed; useful tracks keep their connections and exact heading. Vias touching a same-net pour count as connected on that layer.</p>
      </div>
    </Dialog>
  );
}

// ---------- Via stitching ----------

export function StitchDialog({ docId }: { docId: string }) {
  const doc = useEditor((s) => s.docs[docId]);
  const units = useEditor((s) => s.units);
  const pours = useMemo(() => {
    const out: { id: ItemId; label: string }[] = [];
    for (const [id, it] of doc?.items ?? []) {
      if (it.kind === "zone" && !it.keepout && !it.teardrop && it.net) out.push({ id, label: `${it.net} · ${layer(it.layer).name}${it.name ? ` · ${it.name}` : ""}${it.fill.length ? "" : " (not filled)"}` });
    }
    return out;
  }, [doc?.items]);
  const preselected = [...(doc?.selection ?? [])].find((id) => pours.some((p) => p.id === id));
  const [zone, setZone] = useState<ItemId | null>(preselected ?? pours[0]?.id ?? null);
  const [pitch, setPitch] = useState(Math.round(2.5 * NM_PER_MM));
  const [stagger, setStagger] = useState(true);
  const [bothSides, setBothSides] = useState(true);
  const [plan, setPlan] = useState<{ vias: ViaItem[]; candidates: number; rejected: number } | null>(null);
  const [error, setError] = useState("");
  useEffect(() => {
    if (zone === null) return;
    let live = true;
    setPlan(null);
    setError("");
    api.pcbStitch(docId, { zone, pitch, stagger, bothSides }).then((r) => live && setPlan(r), (e) => live && setError(String(e)));
    return () => {
      live = false;
    };
  }, [docId, zone, pitch, stagger, bothSides, doc?.items]);

  const run = async () => {
    if (!plan?.vias.length) return;
    if (await execute(docId, { op: "route", delete: [], add: plan.vias })) {
      scheduleAnalysis(docId, 0);
      toast.success(`Added ${plan.vias.length} stitching vias`);
      close();
    }
  };
  const v = plan?.vias[0];
  return (
    <Dialog
      title="Via stitching"
      description="Fills a pour with a grid of vias of its net, joining it to the same net's copper on other layers. Vias keep off pads and existing vias, and must pass DRC."
      footer={
        <>
          <Button variant="outline" onClick={close}>Cancel</Button>
          <Button disabled={!plan?.vias.length} onClick={() => void run()}>{plan ? `Add ${plan.vias.length} vias` : "Checking…"}</Button>
        </>
      }
    >
      {pours.length === 0 ? (
        <p className="text-muted-foreground">The board has no polygon pour with a net. Draw one with P G first.</p>
      ) : (
        <div className="grid gap-3">
          <label className="grid gap-1">
            <span className="text-[11px] text-muted-foreground">Pour</span>
            <NativeSelect value={zone ?? ""} onChange={(e) => setZone(Number(e.target.value))}>
              {pours.map((p) => <NativeSelectOption key={p.id} value={p.id}>{p.label}</NativeSelectOption>)}
            </NativeSelect>
          </label>
          <label className="grid gap-1">
            <span className="text-[11px] text-muted-foreground">Grid pitch</span>
            <LengthInput value={pitch} units={units} onCommit={(p) => p > 0 && setPitch(p)} />
          </label>
          <div className="grid">
            <Check label="Stagger every other row" checked={stagger} onChange={setStagger} />
            <Check label="Only where the net is also poured on another layer" checked={bothSides} onChange={setBothSides} />
          </div>
          <p className="text-muted-foreground">
            {error ? <span className="text-err-line">{error}</span>
              : !plan ? "Checking…"
              : <>{plan.vias.length} vias fit ({plan.candidates} grid points inside the pour, {plan.rejected} rejected by DRC){v && <> · Ø{formatLength(v.diameter, units)} / drill {formatLength(v.drill, units)} from the via rules</>}</>}
          </p>
        </div>
      )}
    </Dialog>
  );
}

// ---------- Board statistics ----------

function Row({ k, v }: { k: string; v: React.ReactNode }) {
  return (
    <div className="flex justify-between gap-4 border-b border-line/60 py-0.5">
      <span className="text-muted-foreground">{k}</span>
      <span className="font-mono">{v}</span>
    </div>
  );
}

export function StatsDialog({ docId }: { docId: string }) {
  const doc = useEditor((s) => s.docs[docId]);
  const units = useEditor((s) => s.units);
  const s = useMemo(() => (doc ? boardStats(doc.items, doc.footprints, doc.airwires) : null), [doc]);
  const names = useMemo(() => {
    const m = new Map<number, string>();
    for (const it of doc?.items.values() ?? []) if (it.kind === "board") for (const l of it.stackup.layers) if (l.layer !== null) m.set(l.layer, l.name);
    return m;
  }, [doc?.items]);
  if (!s) return null;
  const len = (nm: number) => formatLength(Math.round(nm), units);
  const copyDrills = () => {
    const rows = ["Tool,Type,Plated,Size,Count", ...s.drills.map((d) => `${d.label},${d.kind},${d.plated ? "yes" : "no"},${d.kind === "slot" ? `${len(d.width)} x ${len(d.height)}` : len(d.width)},${d.count}`)];
    void navigator.clipboard.writeText(rows.join("\n")).then(() => toast.success("Drill table copied as CSV"));
  };
  return (
    <Dialog
      wide
      title="Board statistics"
      description="Counts and sizes of the board as it is now."
      footer={<><Button variant="outline" onClick={copyDrills} disabled={!s.drills.length}>Copy drill table</Button><Button onClick={close}>Close</Button></>}
    >
      <div className="grid grid-cols-2 gap-6">
        <div className="grid content-start">
          <h4 className="mb-1 text-[11px] font-semibold uppercase text-faint">Board</h4>
          <Row k="Size" v={s.size ? `${len(s.size.width)} × ${len(s.size.height)}` : "no outline"} />
          <Row k="Copper layers" v={s.copperLayers} />
          <Row k="Nets" v={s.nets} />
          <Row k="Unrouted connections" v={s.unrouted} />
          <h4 className="mb-1 mt-3 text-[11px] font-semibold uppercase text-faint">Footprints</h4>
          <Row k="Total (top / bottom)" v={`${s.footprints.total} (${s.footprints.top} / ${s.footprints.bottom})`} />
          <Row k="SMD / through-hole / mixed" v={`${s.footprints.smd} / ${s.footprints.tht} / ${s.footprints.mixed}`} />
          <Row k="Locked" v={s.footprints.locked} />
          <Row k="Pads SMD / THT / NPTH" v={`${s.pads.smd} / ${s.pads.tht} / ${s.pads.npth}`} />
          <h4 className="mb-1 mt-3 text-[11px] font-semibold uppercase text-faint">Routing</h4>
          <Row k="Track segments / arcs" v={`${s.tracks.segments} / ${s.tracks.arcs}`} />
          <Row k="Track length" v={len(s.tracks.length)} />
          {[...s.tracks.byLayer].sort((a, b) => a[0] - b[0]).map(([l, n]) => <Row key={l} k={`  on ${names.get(l) ?? layer(l).name}`} v={len(n)} />)}
          <Row k="Vias" v={(Object.keys(s.vias) as ViaKind[]).filter((k) => s.vias[k]).map((k) => `${s.vias[k]} ${VIA_KIND_LABEL[k].toLowerCase()}`).join(", ") || "0"} />
          <Row k="Pours / keep-outs / teardrops" v={`${s.zones.pours} / ${s.zones.keepouts} / ${s.zones.teardrops}`} />
        </div>
        <div className="grid content-start">
          <h4 className="mb-1 text-[11px] font-semibold uppercase text-faint">Drill table</h4>
          {s.drills.length === 0 ? (
            <p className="text-muted-foreground">No holes.</p>
          ) : (
            <div className="rounded-[5px] border border-line">
              <div className="grid grid-cols-[40px_1fr_70px_1fr_60px] gap-2 border-b border-line bg-panel-head px-2 py-1 text-[11px] font-semibold text-faint">
                <span>Tool</span><span>Type</span><span>Plated</span><span>Size</span><span className="text-right">Count</span>
              </div>
              {s.drills.map((d) => (
                <div key={d.label} className="grid grid-cols-[40px_1fr_70px_1fr_60px] gap-2 border-b border-line/60 px-2 py-0.5 font-mono">
                  <span>{d.label}</span>
                  <span className="font-sans">{d.kind === "via" ? "Via" : d.kind === "slot" ? "Slot" : "Pad"}</span>
                  <span className="font-sans">{d.plated ? "PTH" : "NPTH"}</span>
                  <span>{d.kind === "slot" ? `${len(d.width)} × ${len(d.height)}` : len(d.width)}</span>
                  <span className="text-right">{d.count}</span>
                </div>
              ))}
              <div className="flex justify-between px-2 py-1 text-[11px] text-faint">
                <span>{s.drills.length} tools</span>
                <span>{s.drills.reduce((n, d) => n + d.count, 0)} holes</span>
              </div>
            </div>
          )}
        </div>
      </div>
    </Dialog>
  );
}

// ---------- Teardrops ----------

export function TeardropDialog({ docId }: { docId: string }) {
  const doc = useEditor((s) => s.docs[docId]);
  const units = useEditor((s) => s.units);
  const board = [...doc.items].find(([,item])=>item.kind==="board");
  const savedOptions = board?.[1].kind==="board" ? board[1].autoTeardrops : null;
  const [automatic,setAutomatic]=useState(!!savedOptions);
  const [opts, setOpts] = useState<TeardropOptions>(savedOptions ?? {
    vias: true,
    pthPads: true,
    smdPads: false,
    lengthRatio: 0.5,
    widthRatio: 1,
    maxLength: NM_PER_MM,
    maxWidth: 2 * NM_PER_MM,
    curved: true,
    maxTrackRatio: 0.9,
  });
  const [plan, setPlan] = useState<{ delete: ItemId[]; add: Item[]; vias: number; pads: number } | null>(null);
  useEffect(() => {
    let live = true;
    setPlan(null);
    api.pcbTeardrops(docId, opts).then((r) => live && setPlan(r), report);
    return () => {
      live = false;
    };
  }, [docId, opts, doc?.items]);
  const existing = plan?.delete.length ?? 0;
  const set = (p: Partial<TeardropOptions>) => setOpts({ ...opts, ...p });
  const commit = async (remove: boolean) => {
    if (!plan) return;
    if (await execute(docId, { op: "route", delete: automatic && !remove ? [] : plan.delete, add: remove || automatic ? [] : plan.add, replace: board ? [{id:board[0],item:{...board[1],autoTeardrops:!remove && automatic ? opts : null} as Item}] : [] })) {
      scheduleAnalysis(docId, 0);
      toast.success(remove ? `Removed ${existing} teardrops` : `${plan.add.length} teardrops (${plan.vias} on vias, ${plan.pads} on pads)`);
      close();
    }
  };
  const pct = (k: "lengthRatio" | "widthRatio") => (
    <NativeSelect value={String(opts[k])} onChange={(e) => set({ [k]: Number(e.target.value) })}>
      {[0.3, 0.4, 0.5, 0.6, 0.75, 0.8, 1].map((r) => <NativeSelectOption key={r} value={String(r)}>{Math.round(r * 100)} % of pad</NativeSelectOption>)}
    </NativeSelect>
  );
  return (
    <Dialog
      title="Teardrops"
      description="Flared copper where tracks meet pads and vias. They fill like pours (solid, top priority) and are rebuilt as a set; run again after editing the routing."
      footer={
        <>
          <Button variant="outline" className="mr-auto" disabled={!existing} onClick={() => void commit(true)}>Remove all ({existing})</Button>
          <Button variant="outline" onClick={close}>Cancel</Button>
          <Button disabled={!plan || (!plan.add.length && !existing)} onClick={() => void commit(false)}>{plan ? `${existing ? "Update" : "Add"} (${plan.add.length})` : "Checking…"}</Button>
        </>
      }
    >
      <div className="grid gap-3">
        <div className="grid">
          <Check label="Update automatically after routing and connected moves" checked={automatic} onChange={setAutomatic} />
          <Check label="Vias" checked={opts.vias} onChange={(vias) => set({ vias })} count={plan?.vias} />
          <Check label="Through-hole pads" checked={opts.pthPads} onChange={(pthPads) => set({ pthPads })} />
          <Check label="SMD pads (round or near-square)" checked={opts.smdPads} onChange={(smdPads) => set({ smdPads })} />
          <Check label="Curved flanks" checked={opts.curved} onChange={(curved) => set({ curved })} />
        </div>
        <div className="grid grid-cols-2 gap-3">
          <label className="grid gap-1"><span className="text-[11px] text-muted-foreground">Length beyond the pad edge</span>{pct("lengthRatio")}</label>
          <label className="grid gap-1"><span className="text-[11px] text-muted-foreground">Width at the pad</span>{pct("widthRatio")}</label>
          <label className="grid gap-1"><span className="text-[11px] text-muted-foreground">Max length</span><LengthInput value={opts.maxLength} units={units} onCommit={(maxLength) => maxLength > 0 && set({ maxLength })} /></label>
          <label className="grid gap-1"><span className="text-[11px] text-muted-foreground">Max width</span><LengthInput value={opts.maxWidth} units={units} onCommit={(maxWidth) => maxWidth > 0 && set({ maxWidth })} /></label>
        </div>
        <p className="text-muted-foreground">{plan ? `${plan.add.length} joints get a teardrop (${plan.vias} vias, ${plan.pads} pads)${existing ? ` · replaces ${existing} existing` : ""}. Tracks wider than 90 % of the pad are skipped.` : "Checking…"}</p>
      </div>
    </Dialog>
  );
}
