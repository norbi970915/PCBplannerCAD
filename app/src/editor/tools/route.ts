import {cachedRoutingDefaults,loadRoutingDefaults} from "../routingDefaults";
// Interactive routing: the backend router computes each preview on mouse move;
// clicks commit segments. "*" drops a via and switches copper layer, Space
// opens the via layer picker; / flips the posture, Shift+R cycles walkaround → shove → ignore.

import { api } from "../../core/api";
import { report } from "../../core/actions";
import { toast } from "sonner";
import { L, layer as layerDef } from "../../core/layers";
import { routeModeOf, useEditor } from "../../core/store";
import type { BoardItem, Item, ItemId, ItemOf, Point, RouteMode, TrackItem, ViaItem } from "../../core/types";
import { NM_PER_MM } from "../../core/units";
import { constrain45 } from "../polyedit";
import { samePoint } from "../shapes";
import { place } from "../draw";
import { ringHandles, ringLines } from "./polygon";
import type { Tool, ToolEnv } from "./types";

import { snapRouteDirection } from "../routeDirection";
import { prospectiveLength } from "../routeLengths";

const MODES: RouteMode[] = ["walkaround", "shove", "ignore"];

export function boardSettings(env: ToolEnv): BoardItem | undefined {
  for (const it of env.doc().items.values()) if (it.kind === "board") return it;
  return undefined;
}

export function classOf(board: BoardItem | undefined, net: string) {
  return board?.netClasses.find((c) => c.nets.includes(net)) ?? board?.netClasses[0];
}

/** Copper layers top → bottom from the board stack. */
export function copperLayers(board: BoardItem | undefined): number[] {
  const cu = board?.stackup.layers.filter((l) => l.kind === "copper" && l.layer !== null).map((l) => l.layer as number);
  return cu?.length ? cu : [L.topCopper, L.bottomCopper];
}

/** W / Shift+W steps: the net-class widths plus a standard series, ascending. */
export function routeWidths(board: BoardItem | undefined): number[] {
  const series = [0.1, 0.127, 0.15, 0.2, 0.25, 0.3, 0.4, 0.5, 0.75, 1, 1.5, 2].map((v) => Math.round(v * NM_PER_MM));
  return [...new Set([...series, ...(board?.netClasses.map((c) => c.trackWidth) ?? [])])].sort((a, b) => a - b);
}

/** Explicit via target, otherwise next/previous copper in physical stack order. */
export function routeViaLayer(copper:readonly number[],from:number,preferred:number|null,reverse=false):number|null {
  const layers=[...new Set(copper)];
  if(layers.length<2)return null;
  if(preferred!==null&&preferred!==from&&layers.includes(preferred))return preferred;
  const at=layers.indexOf(from);
  return layers[((at<0?0:at)+(reverse?-1:1)+layers.length)%layers.length];
}

function trackWidth(env: ToolEnv, net: string, pair = false): number {
  const netClass=classOf(boardSettings(env),net);
  const resolved=cachedRoutingDefaults(boardSettings(env),net,env.doc().activeLayer);
  const preferred=resolved?(pair?resolved.pairWidth:resolved.width):(pair ? netClass?.diffPairWidth : netClass?.trackWidth);
  return env.placement().trackWidth ?? (preferred && preferred>0 ? preferred : netClass?.trackWidth) ?? Math.round(0.25 * NM_PER_MM);
}

/** The active net can have a different segment count from its paired net. */
export function routeEnd(tracks:readonly TrackItem[],net:string):Point|undefined {
  for(let i=tracks.length-1;i>=0;i--)if(tracks[i].net===net)return tracks[i].b;
  return undefined;
}

interface Session {
  negativeFrom?: Point;
  net: string;
  layer: number;
  from: Point;
  preview: TrackItem[];
  /** Other-net tracks pushed aside by the shove router (replacements keyed by original id). */
  moved: { id: number; item: Item }[];
  reached: boolean;
  /** Footprint pad / copper end points of the net (route ends there automatically). */
  targets: Point[];
  message: string;
  blockedAt: Point | null;
}

export function routingAnchor(env: ToolEnv, point: Point, hit: [number, string] | null): Point {
  const item = hit ? env.doc().items.get(hit[0]) : undefined;
  if (item?.kind === "via" || item?.kind === "pad") return item.pos;
  if (item?.kind === "footprint") {
    const fp = env.doc().footprints[item.footprint];
    const pads = fp?.items.filter((p): p is ItemOf<"pad"> => p.kind === "pad" && (item.padNets[p.number] ?? "") === hit![1]);
    const centers = pads?.map(p => place(p.pos, { pos: item.pos, rotation: item.rotation, mirror: item.side === "bottom" })) ?? [];
    const center = centers.sort((a,b) => Math.hypot(a.x-point.x,a.y-point.y)-Math.hypot(b.x-point.x,b.y-point.y))[0] ?? point;
    return {x:Math.round(center.x),y:Math.round(center.y)};
  }
  if (item?.kind === "track") {
    const dx=item.b.x-item.a.x, dy=item.b.y-item.a.y;
    const t=Math.max(0,Math.min(1,((point.x-item.a.x)*dx+(point.y-item.a.y)*dy)/(dx*dx+dy*dy || 1)));
    return {x:Math.round(item.a.x+t*dx),y:Math.round(item.a.y+t*dy)};
  }
  return point;
}

/** Actual copper width at a routing start, independent of rule-preferred widths. */
export function routingStartWidth(env:ToolEnv,point:Point,hit:[number,string]|null,layer:number):number|null {
 if(!hit)return null;
 const item=env.doc().items.get(hit[0]);
 const usable=(width:number)=>Number.isSafeInteger(width)&&width>0;
 if((item?.kind==='track'||item?.kind==='arcTrack')&&item.layer===layer&&usable(item.width))return item.width;
 if(!item||!['via','footprint','pad'].includes(item.kind))return null;
 let reach=item.kind==='via'?item.diameter/2:item.kind==='pad'?Math.max(item.width,item.height)/2:0;
 if(item.kind==='footprint'){
  const geom=env.doc().footprints[item.footprint];
  for(const pad of geom?.items??[]){if(pad.kind!=='pad'||item.padNets[pad.number]!==hit[1])continue;const pos=place(pad.pos,{pos:item.pos,rotation:item.rotation,mirror:item.side==='bottom'});if(Math.hypot(pos.x-point.x,pos.y-point.y)<2)reach=Math.max(reach,Math.max(pad.width,pad.height)/2);}
 }
 const widths=new Set<number>();
 for(const track of env.doc().items.values()){
  if(track.kind!=='track'||track.layer!==layer||track.net!==hit[1]||!usable(track.width))continue;
  const dx=track.b.x-track.a.x,dy=track.b.y-track.a.y;
  const u=Math.max(0,Math.min(1,((point.x-track.a.x)*dx+(point.y-track.a.y)*dy)/(dx*dx+dy*dy||1)));
  if(Math.hypot(point.x-track.a.x-u*dx,point.y-track.a.y-u*dy)<=reach+track.width/2+1)widths.add(track.width);
 }
 return widths.size===1?[...widths][0]:null;
}

export function routeTool(pair = false): Tool {
  let s: Session | null = null;
  let adoptedWidth:{previous:number|null;value:number}|null=null;
  let flip = false;
  let seq = 0;
  let inflight = false;
  let pending: Point | null = null;
  let starting = false;
  let committing = false;
  let viaSession: Session | null = null;
  let closeLayerPicker:(()=>void)|null=null;
  let previewSettled: Promise<void> = Promise.resolve();
  const history: { from:Point; negativeFrom?:Point; layer:number; items:unknown }[] = [];
  let lastPreview: { session: Session; items: unknown; footprints: unknown; epoch: number; key: string } | null = null;

  let defaultsRequest:{board:BoardItem;session:Session;layer:number}|null=null;
  let defaultsFailure:{board:BoardItem;session:Session;layer:number;message:string}|null=null;
  let directionAxis:number|null=null,directionOrigin="";
  const routeCursor=(env:ToolEnv,to:Point)=>{
    if(!s||useEditor.getState().placement.cornerStyle==="any")return to;
    const origin=s.from.x+"/"+s.from.y;if(origin!==directionOrigin){directionOrigin=origin;directionAxis=null;}
    if(s.targets.some(t=>samePoint(t,to)))return to;
    const snapped=snapRouteDirection(s.from,to,6/env.scale(),directionAxis);directionAxis=snapped.axis;return snapped.point;
  };

  const requestPreview = (env: ToolEnv, to: Point) => {
    if (!s) return;
    const board=boardSettings(env);
    if(board&&!cachedRoutingDefaults(board,s.net,s.layer)){
      const session=s;
      if(defaultsFailure?.board===board&&defaultsFailure.session===session&&defaultsFailure.layer===session.layer){session.message=defaultsFailure.message;return;}
      if(defaultsRequest?.board===board&&defaultsRequest.session===session&&defaultsRequest.layer===session.layer)return;
      const request={board,session,layer:session.layer};defaultsRequest=request;session.preview=[];session.message="Loading routing rules…";
      void loadRoutingDefaults(board,session.net,session.layer).then(()=>{if(s===session){session.message="";requestPreview(env,env.cursor());}}).catch(error=>{if(s===session){defaultsFailure={...request,message:String(error)};session.message=String(error);report(error);env.redraw();}}).finally(()=>{if(defaultsRequest===request)defaultsRequest=null;});return;
    }
    to=routeCursor(env,to);
    const session = s;
    const req = {
      net: session.net,
      layer: session.layer,
      start: session.from,
      end: to,
      width: trackWidth(env, session.net, pair),
      mode: routeModeOf(useEditor.getState()),
      corner: useEditor.getState().placement.cornerStyle,
      ignore: [],
      postureFlip: flip,
    };
    const key = JSON.stringify({req, negativeFrom: session.negativeFrom ?? null});
    const document = env.doc();
    const sourceItems=document.items, sourceFootprints=document.footprints, sourceEpoch=document.epoch;
    if (lastPreview?.session === session && lastPreview.items === document.items && lastPreview.footprints === sourceFootprints && lastPreview.epoch === sourceEpoch && lastPreview.key === key) {
      // Returning to the in-flight endpoint also cancels an obsolete queued endpoint.
      pending = null;
      return;
    }
    if (inflight) { pending = to; return; }
    lastPreview = { session, items: sourceItems, footprints: sourceFootprints, epoch: sourceEpoch, key };
    inflight = true;
    env.redraw();
    const my = ++seq;
    const done = () => {
      inflight = false;
      env.redraw();
      if (pending) {
        const p = pending;
        pending = null;
        requestPreview(env, p);
      }
    };
    const run = pair ? api.pcbRoutePair(env.docId, req, negativeOf(env, session.net), diffGap(env, session.net), session.negativeFrom ?? null).then(([pos, neg]) => {
      if(pos.reached!==neg.reached || !!pos.tracks.length!==!!neg.tracks.length) return {...pos,reached:false,blockedAt:pos.blockedAt??neg.blockedAt??session.from,message:"Incomplete pair plan; move the cursor to retry",tracks:[],moved:[]};
      return {...pos,reached:pos.reached&&neg.reached,blockedAt:pos.blockedAt??neg.blockedAt,message:(!pos.reached?pos.message:neg.message)||pos.message,tracks:[...pos.tracks,...neg.tracks],moved:pos.tracks.length?pos.moved:[]};
    }) : api.pcbRoute(env.docId, req);
    previewSettled = run
      .then((r) => {
        if (my !== seq || s !== session) return;
        if(env.doc().items!==sourceItems || env.doc().footprints!==sourceFootprints || env.doc().epoch!==sourceEpoch
          || routeModeOf(useEditor.getState())!==req.mode || useEditor.getState().placement.cornerStyle!==req.corner || trackWidth(env,session.net,pair)!==req.width || flip!==req.postureFlip){lastPreview=null;pending=env.cursor();return;}
        const hadMoved = session.moved.length > 0;
        session.preview = r.tracks;
        session.moved = r.moved;
        session.reached = r.reached;
        session.message = r.message;
        session.blockedAt = r.blockedAt;
        env.redraw(hadMoved || r.moved.length > 0);
      })
      .catch((error) => {
        if (s !== session || my !== seq) return;
        lastPreview = null;
        session.preview = [];
        session.moved = [];
        session.message = String(error);
        session.reached = false;
        report(error);
        env.redraw();
      })
      .finally(done);
  };

  const netTargets = (env: ToolEnv, net: string): Point[] => {
    if (!net) return [];
    const doc = env.doc();
    const out: Point[] = [];
    for (const it of doc.items.values()) {
      if (it.kind !== "footprint") continue;
      const geom = doc.footprints[it.footprint];
      if (!geom) continue;
      for (const p of geom.items) {
        if (p.kind !== "pad" || it.padNets[p.number] !== net) continue;
        const pl = { pos: it.pos, rotation: it.rotation, mirror: it.side === "bottom" };
        const m = pl.mirror ? { x: -p.pos.x, y: p.pos.y } : p.pos;
        const a = (pl.rotation * Math.PI) / 180;
        out.push({ x: Math.round(pl.pos.x + m.x * Math.cos(a) - m.y * Math.sin(a)), y: Math.round(pl.pos.y + m.x * Math.sin(a) + m.y * Math.cos(a)) });
      }
    }
    return out;
  };

  const commit = async (env: ToolEnv, transition?: { to: Point; target: number }): Promise<boolean> => {
    if (!s || committing) return false;
    const session = s;
    committing = true;
    try {
      requestPreview(env, transition?.to ?? env.cursor());
      // Commit the clicked endpoint, never an older asynchronous mouse preview.
      while (inflight || pending) await previewSettled;
      if (s !== session || (!session.preview.length && !transition)) return false;
      // A blocked/partial route must never drop a via back at its previous source.
      if (transition && (!session.reached || !samePoint((pair ? routeEnd(session.preview,session.net) : session.preview.at(-1)?.b) ?? session.from, transition.to))) return false;
      let tracks = session.preview;
      const moved = session.moved;
      let vias:ViaItem[]=[];
      if(transition){
        const document=env.doc(),items=document.items,footprints=document.footprints,epoch=document.epoch;
        const settings=()=>JSON.stringify([trackWidth(env,session.net,pair),diffGap(env,session.net),routeModeOf(useEditor.getState()),useEditor.getState().placement.cornerStyle,flip]);
        const plannedSettings=settings();
        const other=pair?routeEnd(tracks,negativeOf(env,session.net)):undefined;
        if(pair&&!other)return false;
        const active=await api.pcbVia(env.docId,session.net,transition.to,session.layer,transition.target);
        if(s!==session)return false;
        if(pair&&other){
          const partner=await api.pcbVia(env.docId,negativeOf(env,session.net),other,session.layer,transition.target);
          const last=tracks.filter(t=>t.net===session.net).at(-1);
          if(s!==session)return false;
          const fanout=await api.pcbPairViaPlan(env.docId,{positive:active,negative:partner,source:session.layer,target:transition.target,direction:{x:transition.to.x-(last?.a.x??session.from.x),y:transition.to.y-(last?.a.y??session.from.y)},width:trackWidth(env,session.net,pair),corner:useEditor.getState().placement.cornerStyle,mode:routeModeOf(useEditor.getState()),tracks,moved});
          tracks=fanout.tracks;vias=fanout.vias;
        }else vias=[active];
        if(s!==session||env.doc().items!==items||env.doc().footprints!==footprints||env.doc().epoch!==epoch||settings()!==plannedSettings){session.message='Board or routing settings changed while planning vias; try again.';return false;}
      }
      seq++;
      pending = null;
      const ok = await env.exec({ op: "route", delete: [...new Set(moved.map(m => m.id))], add: [...moved.map(m => m.item), ...tracks, ...vias] });
      if (s !== session) return false;
      if (!ok) {
        if (transition) {
          session.reached = false;
          session.blockedAt = transition.to;
          session.message = "Via placement rejected; see Messages for the rule and layer. Move to a clear position to retry.";
        }
        return false;
      }
      history.push({ from:{...session.from}, negativeFrom:session.negativeFrom, layer:session.layer, items:env.doc().items });
      if(pair)session.negativeFrom=routeEnd(tracks,negativeOf(env,session.net));
      session.from = routeEnd(tracks, session.net) ?? session.from;
      session.preview = [];
      session.moved = [];
      session.message = "";
      session.blockedAt = null;
      // Keep the freshly committed copper lit with the rest of the net.
      showNet(env, session.net);
      return true;
    } catch(error) {
      if(s===session){session.reached=false;session.message=String(error);report(error);env.redraw();}
      return false;
    } finally { committing = false; }
  };

  // While routing, the net's objects stay lit and the rest of the board fades; the earlier highlight returns afterwards.
  let netHighlight: { docId: string; previous: ReadonlySet<ItemId> } | null = null;
  /** Pad the route started from; its own target is not marked as an "other end". */
  let startAnchor: Point | null = null;
  const showNet = (env: ToolEnv, net: string) => {
    if (!net) return;
    netHighlight ??= { docId: env.docId, previous: env.doc().marked ?? new Set() };
    const session = s;
    void api.pcbNetItems(env.docId, net)
      .then((ids) => { if (s === session && netHighlight) useEditor.getState().patchDoc(env.docId, { marked: new Set(ids) }); })
      .catch(() => { /* The highlight is a visual aid only. */ });
  };
  /** Target pads still to reach, nearest to `from` first. */
  const otherEnds = (session: Session, from: Point): Point[] =>
    session.targets
      .filter((t) => !(startAnchor && samePoint(t, startAnchor)) && !samePoint(t, session.from))
      .sort((a, b) => Math.hypot(a.x - from.x, a.y - from.y) - Math.hypot(b.x - from.x, b.y - from.y));

  const finish = () => {
    if(adoptedWidth&&useEditor.getState().placement.trackWidth===adoptedWidth.value)useEditor.getState().setPlacement({trackWidth:adoptedWidth.previous});
    adoptedWidth=null;
    s = null;defaultsRequest=null;defaultsFailure=null;
    closeLayerPicker?.();closeLayerPicker=null;
    viaSession = null;
    history.length = 0;
    lastPreview = null;
    pending = null;
    starting = false;
    startAnchor = null;
    seq++;
    if (netHighlight) {
      useEditor.getState().patchDoc(netHighlight.docId, { marked: netHighlight.previous });
      netHighlight = null;
    }
  };

  return {
    id: pair ? "diffPair" : "route",
    hint: (env) =>
      s
        ? `Routing ${s.net || "(no net)"} on ${s.layer === L.bottomCopper ? "bottom" : s.layer === L.topCopper ? "top" : "inner"}${s.net ? ` · ${otherEnds(s, s.from).length} other ends (cyan rings)` : ""} · width ${(trackWidth(env, s.net, pair) / NM_PER_MM).toFixed(3)} mm (W / Shift+W) · click to commit · Backspace removes last step · Space chooses via layer · V quick via · / flip corner · ${useEditor.getState().drcEnforce ? `Shift+R mode (${useEditor.getState().placement.routeMode})` : "design rules OFF: ignoring violators (Ctrl+Shift+D)"} · Esc to select${s.message ? ` · ${s.reached ? "" : "Blocked: "}${s.message}` : ""}`
        : `Click a pad or track to start ${pair ? "a differential pair" : "routing"}`,
    reset: finish,
    down(env) {
      if (viaSession) return;
      const p = env.cursor();
      if (!s) {
        if (starting) return;
        starting = true;
        const generation = ++seq;
        const layer = env.doc().activeLayer;
        const copper = copperLayers(boardSettings(env));
        const active = copper.includes(layer) ? layer : L.topCopper;
        const mouse = env.raw();
        const raw = {x:Math.round(mouse.x),y:Math.round(mouse.y)};
        const at = async (l: number) => (await api.pcbObjectAt(env.docId, raw, l)) ?? (samePoint(raw, p) ? null : await api.pcbObjectAt(env.docId, p, l));
        // Start on the layer of what was clicked: a pad (active layer first), then a track or via
        // (active layer first). Pours never switch layers, so clicking open board does not jump to a plane.
        // E.g. an SMD pad of a bottom-side part while Top is active starts on Bottom.
        const find = async (): Promise<{ hit: [number, string] | null; layer: number }> => {
          const hits: { hit: [number, string]; layer: number; kind?: string }[] = [];
          for (const l of [active, ...copper.filter((c) => c !== active)]) {
            if (generation !== seq) break;
            const hit = await at(l);
            if (hit) hits.push({ hit, layer: l, kind: env.doc().items.get(hit[0])?.kind });
          }
          const pad = hits.find((h) => h.kind === "footprint");
          const onActive = hits.find((h) => h.layer === active);
          const copperHit = hits.find((h) => h.kind === "track" || h.kind === "arcTrack" || h.kind === "via");
          return pad ?? (onActive && onActive.kind !== "zone" ? onActive : copperHit) ?? onActive ?? { hit: null, layer: active };
        };
        void find()
          .then(({ hit, layer: routeLayer }) => {
            if (generation !== seq) return;
            starting = false;
            if (routeLayer !== env.doc().activeLayer) {
              useEditor.getState().patchDoc(env.docId, { activeLayer: routeLayer });
              toast.info(`Routing on ${layerDef(routeLayer).name}: the clicked ${env.doc().items.get(hit?.[0] ?? -1)?.kind === "footprint" ? "pad" : "object"} is on that layer.`, { duration: 2500 });
            }
            const net = hit?.[1] ?? useEditor.getState().placement.net;
            if (pair && (!net || negativeOf(env,net)===net)) { toast.warning("Choose a configured differential pair or a net with paired naming before routing a pair."); return; }
            const from = routingAnchor(env, p, hit);
            const inherited=routingStartWidth(env,from,hit,routeLayer);
            if(inherited!==null){adoptedWidth={previous:env.placement().trackWidth,value:inherited};env.setPlacement({trackWidth:inherited});}
            s = { net, layer: routeLayer, from, preview: [], moved: [], reached: true, targets: netTargets(env, net), message: "", blockedAt: null };
            startAnchor = from;
            showNet(env, net);
            requestPreview(env, env.cursor());
            env.redraw();
          })
          .catch(error => { if (generation === seq) { starting = false; report(error); } });
        return;
      }
      void commit(env).then((ok) => {
        if (ok && s && s.targets.some((t) => samePoint(t, p))) finish();
        env.redraw();
      });
    },
    move(env) {
      if (s && !committing && viaSession !== s) requestPreview(env, env.cursor());
    },
    dblclick(env) {
      void commit(env).then(() => {
        finish();
        env.redraw();
      });
    },
    key(env, key, e) {
      if (!s) return false;
      if (key === "Backspace") {
        if (committing || viaSession) return true;
        const session=s, previous=history.at(-1);
        if (!previous) { session.message="No committed routing step to remove"; env.redraw(); return true; }
        if (previous.items !== env.doc().items) { session.message="Board changed outside this route; finish routing before using Undo"; env.redraw(); return true; }
        committing=true;
        seq++; pending=null; lastPreview=null;
        void env.undo().then(ok=>{
          if (!ok || s!==session) return;
          history.pop();
          const last=history.at(-1); if(last)last.items=env.doc().items;
          session.from=previous.from;session.negativeFrom=previous.negativeFrom;session.layer=previous.layer;
          session.preview=[];session.moved=[];session.message="";session.blockedAt=null;session.reached=true;
          useEditor.getState().patchDoc(env.docId,{activeLayer:session.layer});
          showNet(env,session.net);requestPreview(env,env.cursor());
        }).catch(report).finally(()=>{committing=false;env.redraw(true);});
        return true;
      }
      if (key === " " && e.shiftKey) {
        const corners = ["diagonal45", "orthogonal90", "any"] as const;
        const current = useEditor.getState().placement.cornerStyle;
        env.setPlacement({ cornerStyle: corners[(corners.indexOf(current as typeof corners[number]) + 1) % corners.length] });
        requestPreview(env, env.cursor());
        return true;
      }
      if (key === "0") {
        env.setPlacement({ trackWidth: null });
        requestPreview(env, env.cursor());
        return true;
      }
      if (key === "/") {
        flip = !flip;
        requestPreview(env, env.cursor());
        return true;
      }
      if (key === "w" || key === "W") {
        const widths = routeWidths(boardSettings(env));
        const current = trackWidth(env, s.net, pair);
        const next = e.shiftKey ? [...widths].reverse().find((w) => w < current) ?? widths[0] : widths.find((w) => w > current) ?? widths[widths.length - 1];
        env.setPlacement({ trackWidth: next });
        requestPreview(env, env.cursor());
        return true;
      }
      if ((key === "R" || key === "r") && e.shiftKey) {
        const m = useEditor.getState().placement.routeMode;
        env.setPlacement({ routeMode: MODES[(MODES.indexOf(m) + 1) % MODES.length] });
        requestPreview(env, env.cursor());
        return true;
      }
      if (key === " " || key === "*" || key === "v" || key === "V") {
        if (viaSession === s || committing) return true;
        const session = s;
        const to = routeCursor(env,env.cursor());
        const cu = copperLayers(boardSettings(env));
        const next = routeViaLayer(cu, session.layer, env.placement().routeViaLayer ?? null, e.shiftKey);
        if (next === null) { session.message = "A via needs two copper layers in the stack"; env.redraw(); return true; }
        viaSession = session;
        void (async () => {
          let target=next;
          if(key===' '){
            const before=env.doc().items,board=boardSettings(env);
            closeLayerPicker=()=>env.dismissRouteLayer();
            const chosen=await env.chooseRouteLayer({current:session.layer,suggested:next,layers:cu.map(id=>({id,name:board?.stackup.layers.find(layer=>layer.layer===id)?.name||layerDef(id).name,color:layerDef(id).color}))});
            closeLayerPicker=null;
            if(s!==session||chosen===null)return;
            if(env.doc().items!==before){session.message='Board changed while choosing a via layer; try again.';return;}
            if(chosen===session.layer||!cu.includes(chosen))return;
            target=chosen;
          }
          if(s!==session)return;
          // Both pair necks, vias, copper and refill share one transaction and Undo.
          if (!await commit(env, { to, target }) || s !== session) return;
          session.layer = target;
          useEditor.getState().patchDoc(env.docId, { activeLayer: session.layer });
          requestPreview(env, env.cursor());
        })().catch(report).finally(() => { if (viaSession === session) viaSession = null; env.redraw(); });
        return true;
      }
      return false;
    },
    cancel() {
      if (!s && !starting) return false;
      finish();
      return true;
    },
    routingMetrics(env) {
      if(!s)return null;
      const nets=pair?[s.net,negativeOf(env,s.net)]:[s.net];
      return {nets:nets.map(net=>({net,length:prospectiveLength(env.doc().items,net,s!.preview,s!.moved)})),pending:inflight||pending!==null,blocked:s.blockedAt!==null||(!s.reached&&!!s.message)};
    },
    preview: () => (s ? [...s.moved.map((m) => m.item), ...s.preview] : []),
    excluded: () => new Set(s?.moved.map((m) => m.id) ?? []),
    overlay: env => {
      if (!s) return [];
      const width = 1 / env.scale();
      const lines:import("../renderer").DrawLine[] = [];
      const mark = (p: Point, color: [number,number,number,number]) => {
        const r=5/env.scale();
        lines.push({a:{x:p.x-r,y:p.y},b:{x:p.x+r,y:p.y},width,color});
        lines.push({a:{x:p.x,y:p.y-r},b:{x:p.x,y:p.y+r},width,color});
      };
      lines.push({a:s.from,b:s.from,width:3/env.scale(),color:[.65,.95,.85,.9]});
      if (!s.reached && s.blockedAt) mark(s.blockedAt,[1,0.3,0.25,1]);
      // The other ends of the net: a ring on each target pad and a guide from the route's end to the nearest one.
      const end = routeEnd(s.preview,s.net) ?? s.from;
      const targets = otherEnds(s, end);
      const cyan: [number, number, number, number] = [0.5, 0.8, 0.95, 0.6];
      const ring = 5 / env.scale();
      for (const t of targets.slice(0,4)) {
        for (let i = 0; i < 16; i++) {
          const a0 = (i / 16) * 2 * Math.PI, a1 = ((i + 1) / 16) * 2 * Math.PI;
          lines.push({ a: { x: t.x + ring * Math.cos(a0), y: t.y + ring * Math.sin(a0) }, b: { x: t.x + ring * Math.cos(a1), y: t.y + ring * Math.sin(a1) }, width, color: cyan });
        }
      }
      if (targets.length) lines.push({ a: end, b: targets[0], width: width * 0.8, color: [0.5, 0.8, 0.95, 0.3] });
      return lines;
    },
  };
}

function negativeOf(env: ToolEnv, net: string): string {
  const pairs = boardSettings(env)?.diffPairs ?? [];
  const p = pairs.find((d) => d.positive === net || d.negative === net);
  if (p) return p.positive === net ? p.negative : p.positive;
  // Common naming conventions: USB_P/USB_N, D+/D-, CLK_P/CLK_N.
  for (const [a, b] of [
    ["_P", "_N"],
    ["+", "-"],
    ["_DP", "_DN"],
    ["P", "N"],
  ]) {
    if (net.endsWith(a)) return net.slice(0, -a.length) + b;
    if (net.endsWith(b)) return net.slice(0, -b.length) + a;
  }
  return net;
}

function diffGap(env: ToolEnv, net: string): number {
  return cachedRoutingDefaults(boardSettings(env),net,env.doc().activeLayer)?.pairGap ?? (classOf(boardSettings(env), net)?.diffPairGap || Math.round(0.2 * NM_PER_MM));
}

// ---------- polygon-point tools (zones, keep-outs, board outline) ----------

export function polygonTool(id: "zone" | "keepout" | "outline"): Tool {
  let pts: Point[] = [];
  let shift = false;
  /** Next corner: grid cursor, 45° constrained with Shift, or the first corner when close to it. */
  const next = (env: ToolEnv): { p: Point; closes: boolean } => {
    const c = env.cursor();
    if (pts.length >= 3) {
      const r = env.raw();
      if (Math.hypot(r.x - pts[0].x, r.y - pts[0].y) <= 8 / env.scale()) return { p: pts[0], closes: true };
    }
    return { p: shift && pts.length ? constrain45(pts[pts.length - 1], c) : c, closes: false };
  };
  const build = (env: ToolEnv, points: Point[]): Item | null => {
    if (points.length < 2) return null;
    const pl = env.placement();
    const layer = env.doc().activeLayer;
    if (id === "outline") return { kind: "polyline", points, closed: points.length > 2, width: Math.round(0.1 * NM_PER_MM), layer: L.boardOutline, fill: "none" };
    const zone: ItemOf<"zone"> = {
      kind: "zone",
      outline: points,
      layer: layer === L.multi ? L.topCopper : layer,
      net: id === "keepout" ? "" : pl.net,
      name: "",
      priority: pl.zonePriority,
      // The pour keeps the larger of this and the clearance rules.
      clearance: id === "keepout" ? 0 : Math.round(0.3 * NM_PER_MM),
      minWidth: Math.round(0.2 * NM_PER_MM),
      connect: "thermal",
      thermalGap: Math.round(0.3 * NM_PER_MM),
      spokeWidth: Math.round(0.3 * NM_PER_MM),
      keepout: id === "keepout",
      removeIslands: true,
      fill: [],
    };
    return zone;
  };
  const close = (env: ToolEnv) => {
    const corners = pts;
    const item = corners.length >= 3 ? build(env, corners) : null;
    pts = [];
    if (!item) return;
    void (async () => {
      if (item.kind === "zone" && !item.keepout && !item.net) item.net = await pourNet(env, corners[0], item.layer);
      return env.exec({ op: "add", items: [item] });
    })().then(async (ok) => {
      if (!ok || item.kind !== "zone" || item.keepout) return;
      // Pour right away so the user sees the copper.
      const { repour } = await import("../../core/pcbActions");
      await repour(env.docId);
    });
  };
  const label = id === "zone" ? "copper pour" : id === "keepout" ? "keep-out area" : "board outline";
  /** No net chosen: take the copper under the first corner, else GND. */
  const pourNet = async (env: ToolEnv, at: Point, layer: number): Promise<string> => {
    try {
      const hit = await api.pcbObjectAt(env.docId, at, layer);
      if (hit?.[1]) return hit[1];
      const nets = await api.pcbNets(env.docId);
      const gnd = nets.find((n) => n.toUpperCase() === "GND") ?? nets.find((n) => /^(A|D|P)?GND/i.test(n));
      if (gnd) {
        useEditor.getState().log("info", `New pour connected to ${gnd} — change the net in Properties if needed.`);
        return gnd;
      }
    } catch {
      // Fall through: a pour without a net is still valid (shielding).
    }
    useEditor.getState().log("warning", "The new pour has no net — pick one in Properties so it connects.");
    return "";
  };
  return {
    id,
    hint: () =>
      pts.length
        ? `Click to add corners of the ${label} · click the first corner, double-click to close · right-drag pans · Shift: 45° · Backspace removes a corner`
        : `Click to start the ${label}`,
    reset() {
      pts = [];
    },
    move(_env, e) {
      shift = e.shift;
    },
    down(env, e) {
      shift = e.shift;
      const { p, closes } = next(env);
      if (closes) return close(env);
      if (pts.length && samePoint(p, pts[pts.length - 1])) return;
      pts.push(p);
    },
    dblclick(env) {
      close(env);
    },
    key(_env, key) {
      if (key === "Backspace" && pts.length) {
        pts.pop();
        return true;
      }
      return false;
    },
    cancel(env) {
      if (!pts.length) return false;
      close(env);
      return true;
    },
    preview: (env) => {
      if (!pts.length) return [];
      const { p, closes } = next(env);
      const it = build(env, closes ? pts : [...pts, p]);
      return it ? [it] : [];
    },
    overlay: (env) => {
      if (!pts.length) return [];
      const px = 1 / env.scale();
      const { p, closes } = next(env);
      return [...ringLines([...pts, p], px, [1, 0.82, 0.25, 1], false, 1.5), ...ringHandles(closes ? [pts[0]] : pts, px, false)];
    },
  };
}

// ---------- footprint placement ----------

export function footprintTool(): Tool {
  const build = (env: ToolEnv, p: Point): Item | null => {
    const pf = useEditor.getState().placement.footprint;
    if (!pf) return null;
    const pl = env.placement();
    return {
      kind: "footprint",
      designator: pf.designator,
      value: pf.value,
      footprint: pf.key,
      pos: p,
      rotation: pl.rotation,
      side: pl.mirror ? "bottom" : "top",
      locked: false,
      padNets: {},
      component: pf.component,
      parameters: {},
    };
  };
  return {
    id: "footprint",
    hint: () => "Click to place the footprint · R rotate · right-drag pan · L flips to the other side · Esc to finish",
    preview: (env) => {
      const it = build(env, env.cursor());
      return it ? [it] : [];
    },
    down(env) {
      const it = build(env, env.cursor());
      if (!it) return;
      void env.exec({ op: "add", items: [it] }).then((ok) => {
        const pf = useEditor.getState().placement.footprint;
        if (ok && pf) env.setPlacement({ footprint: { ...pf, designator: bumpDesignator(pf.designator) } });
      });
    },
    key(env, key) {
      if (key.toLowerCase() === "r") {
        env.setPlacement({ rotation: (env.placement().rotation + 90) % 360 });
        return true;
      }
      if (key === "l" || key === "L" || key === "x" || key === "X") {
        env.setPlacement({ mirror: !env.placement().mirror });
        return true;
      }
      return false;
    },
    cancel: () => false,
  };
}

function bumpDesignator(d: string): string {
  const m = /^(.*?)(\d+)$/.exec(d);
  return m ? `${m[1]}${Number(m[2]) + 1}` : d;
}

// Re-exported for the Properties panel's net pickers.
export type { BoardItem };
