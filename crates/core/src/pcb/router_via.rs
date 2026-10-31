//! Bounded via shoving: move a conflicting via and its connected straight tracks
//! together, check every spanned layer, then retry the route against that world.
use crate::document::{Document, Edit, IdItem};
use crate::item::Item;
use crate::pcb::geometry::{board_settings, world_pads};
use crate::pcb::board::BoardSettings;
use crate::units::Point;
use super::{RouteRequest, RouteResult};
use super::geom::{corner_path, seg_seg_dist, V};

/// Indexed rejection precedes expensive speculative board copies and full DRC.
/// The authoritative final checks still cover holes, keep-outs and board edges.
fn copper_clear(index:&crate::pcb::connectivity::Index,board:&BoardSettings,rules:&crate::pcb::query::RuleSet<'_>,replacements:&[IdItem])->bool{
    let moving:std::collections::HashSet<_>=replacements.iter().map(|entry|entry.id).collect();
    let sample=Document::from_item_list(crate::DocKind::Pcb,replacements.iter().map(|entry|entry.item.clone()));
    for object in crate::pcb::connectivity::build_objects(&sample,board,None,None){
        let geometry=object.geo();
        for other in index.tree.query(geometry.bb.grow(rules.max_clearance() as f64+5.)){
            let fixed=&index.objs[other];
            if moving.contains(&fixed.item)||(!object.net.is_empty()&&object.net==fixed.net)||!object.shares_layer(fixed){continue;}
            for layer in object.layers.iter().filter(|layer|fixed.on_layer(**layer)){
                let clearance=rules.clearance(&object.obj(Some(*layer)),&fixed.obj(Some(*layer)));
                if crate::pcb::connectivity::geo::gap(&geometry,&index.geos[other],clearance as f64+5.).is_some_and(|(gap,_)|gap<clearance as f64-5.){return false;}
            }
        }
    }
    true
}

pub(super) fn route(doc: &Document, req: &RouteRequest) -> Option<RouteResult> {
    let fallback = BoardSettings::default();
    let board = board_settings(doc).map_or(&fallback, |(_,b)| b);
    let path = corner_path(req.start, req.end, req.corner, req.posture_flip);
    // Conservative maximum clearance: the final DRC still decides whether a candidate is legal.
    let clearance = board.net_classes.iter().map(|c|c.clearance).max().unwrap_or(250_000).max(250_000);
    let mut attempted=0;
    let mut index=None;
    let rules=crate::pcb::query::RuleSet::new(board);
    for (id,item) in doc.items() {
        let Item::Via(via)=item else {continue};
        if via.locked || via.net == req.net || req.ignore.contains(&id) || !board.stackup.span(via.from_layer,via.to_layer).contains(&req.layer) {continue;}
        let radius=(via.diameter+req.width) as f64/2.0+clearance as f64;
        if !path.windows(2).any(|w|seg_seg_dist(V::of(w[0]),V::of(w[1]),V::of(via.pos),V::of(via.pos))<radius) {continue;}
        attempted+=1; if attempted>8||super::engine::work_exhausted() {break;}
        let span=board.stackup.span(via.from_layer,via.to_layer);
        // Vias anchored directly on fixed pads, arcs or locked tracks cannot be displaced safely.
        let anchored=doc.items().any(|(fid,i)|match i {
            Item::Footprint(fp)=>doc.cached_footprint(&fp.footprint).is_some_and(|g|world_pads(fid,fp,g,board).iter().any(|p|p.copper.iter().any(|l|span.contains(l)) && ((p.pos.x-via.pos.x)as f64).hypot((p.pos.y-via.pos.y)as f64)<p.width.max(p.height) as f64/2.0)),
            Item::Track(t)=>t.locked && span.contains(&t.layer) && (t.a==via.pos||t.b==via.pos),
            Item::ArcTrack(a)=>span.contains(&a.layer) && [a.start_angle,a.start_angle+a.sweep].iter().any(|angle|{let r=angle.to_radians();Point::new(a.center.x+(a.radius as f64*r.cos()).round()as i64,a.center.y+(a.radius as f64*r.sin()).round()as i64)==via.pos}),
            _=>false,
        });
        if anchored {continue;}
        let step=radius.ceil()as i64+10_000;
        for (dx,dy) in [(0,step),(0,-step),(step,0),(-step,0),(step,step),(-step,-step),(step,-step),(-step,step)] {
            let pos=Point::new(via.pos.x+dx,via.pos.y+dy);
            let mut moved=via.clone();moved.pos=pos;
            let mut replacements=vec![IdItem{id,item:Item::Via(moved)}];
            for (tid,i) in doc.items() {
                let Item::Track(t)=i else {continue};
                if t.net!=via.net||!span.contains(&t.layer)||(t.a!=via.pos&&t.b!=via.pos){continue;}
                let mut shifted=t.clone();if shifted.a==via.pos{shifted.a=pos;}if shifted.b==via.pos{shifted.b=pos;}
                replacements.push(IdItem{id:tid,item:Item::Track(shifted)});
            }
            let index=index.get_or_insert_with(||super::engine::cached_copper(doc));
            if !copper_clear(index,board,&rules,&replacements){continue;}
            let mut work=doc.clone();
            let edits:Vec<_>=replacements.iter().map(|r|Edit{id:r.id,before:work.get(r.id).cloned(),after:Some(r.item.clone())}).collect();work.apply(&edits);
            let ids:Vec<_>=replacements.iter().map(|r|r.id).collect();
            if crate::pcb::drc::run_near(&work,&ids).iter().any(|v|matches!(v.code,"short"|"clearance"|"board-outline"|"keepout"|"via-style"|"hole-size")){continue;}
            let mut routed=super::route_base(&work,req);if !routed.reached{continue;}
            let shoved:std::collections::HashSet<_>=routed.moved.iter().map(|r|r.id).collect();
            replacements.retain(|r|!shoved.contains(&r.id));
            routed.moved.extend(replacements);return Some(routed);
        }
    }
    None
}
