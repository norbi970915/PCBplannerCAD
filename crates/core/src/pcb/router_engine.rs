//! Snapshot-scoped routing state. Spatial indexes are immutable; every solve
//! gets a fresh overlay, and returning to a reviewed target restores its plan.
use std::{cell::RefCell,collections::VecDeque,sync::Arc};
use serde::Serialize;
use crate::{Document,LayerId,ItemId,Point};
use crate::pcb::{elastic::{self,Topology,TrackRequest},drag::{self,DragRequest,DragResult}};
use super::{RouteRequest,RouteResult,PairViaRequest,PairViaPlan,world::WorldIndex};

// One preview shares its work allowance across every launch, window and fallback.
// The count is deterministic and independent of machine load or cache warmth.
const SEARCH_WORK:usize=6_000;
thread_local! {static WORK:RefCell<Option<usize>>=const {RefCell::new(None)};}
pub(crate) struct WorkScope {owner:bool}
impl WorkScope {pub(crate) fn enter()->Self{
    let owner=WORK.with(|work|{let mut work=work.borrow_mut();if work.is_none(){*work=Some(SEARCH_WORK);true}else{false}});Self{owner}
}}
impl Drop for WorkScope {fn drop(&mut self){if self.owner{WORK.with(|work|{work.replace(None);});}}}
pub(crate) fn consume_work(amount:usize)->bool{WORK.with(|work|{
    let mut work=work.borrow_mut();let Some(remaining)=work.as_mut()else{return true};
    if *remaining<amount{*remaining=0;false}else{*remaining-=amount;true}
})}
// A costly speculative guide must leave work for the other pair candidates.
// Unused work is returned to the enclosing operation; spent work is never reset.
pub(crate) struct WorkSlice {saved:Option<(usize,usize)>}
impl WorkSlice {pub(crate) fn enter(limit:usize)->Self{
 let saved=WORK.with(|work|{let mut work=work.borrow_mut();let parent=(*work)?;let allowed=parent.min(limit);*work=Some(allowed);Some((parent,allowed))});Self{saved}
}
/// Keep work available for a dependency fallback. Cheap local corridors still
/// get a small complete attempt when the enclosing allowance is already small.
pub(super) fn reserving(reserve:usize)->Self {
 let limit=WORK.with(|work|(*work.borrow()).map(|parent|parent.saturating_sub(reserve).div_ceil(2).max(8))).unwrap_or(usize::MAX);
 Self::enter(limit)
}}
impl Drop for WorkSlice {fn drop(&mut self){if let Some((parent,allowed))=self.saved{WORK.with(|work|{let mut work=work.borrow_mut();let spent=allowed.saturating_sub(work.unwrap_or(0));*work=Some(parent.saturating_sub(spent));});}}}
pub(crate) fn work_exhausted()->bool{WORK.with(|work|matches!(*work.borrow(),Some(0)))}
const INDEX_LIMIT:usize=8;
const PREVIEW_LIMIT:usize=16;
type IndexKey=(LayerId,Vec<ItemId>);
#[derive(Default)]
struct IndexCache {entries:VecDeque<(IndexKey,WorldIndex)>,hits:u64,builds:u64,copper:Option<Arc<crate::pcb::connectivity::Index>>,pads:Option<Arc<Vec<crate::pcb::geometry::WorldPad>>>}
struct Context {source:usize,cache:IndexCache,topology:Arc<Topology>}
thread_local! {static ACTIVE:RefCell<Option<Context>>=const {RefCell::new(None)};}
fn index_key(layer:LayerId,ignore:&[ItemId])->IndexKey{
    let mut ids=ignore.to_vec();ids.sort();ids.dedup();(layer,ids)
}
pub(super) fn cached_pads(doc:&Document)->Arc<Vec<crate::pcb::geometry::WorldPad>>{
    let cached=ACTIVE.with(|active|active.borrow().as_ref().filter(|context|context.source==doc as *const Document as usize).and_then(|context|context.cache.pads.clone()));
    if let Some(cached)=cached{return cached;}
    let pads=Arc::new(crate::pcb::geometry::all_pads(doc,super::world::board_of(doc)));
    ACTIVE.with(|active|{if let Some(context)=active.borrow_mut().as_mut().filter(|context|context.source==doc as *const Document as usize){context.cache.pads=Some(pads.clone());}});
    pads
}
fn copper_index(doc:&Document)->Arc<crate::pcb::connectivity::Index>{
    let included=doc.items().filter_map(|(id,item)|(!matches!(item,crate::Item::Zone(_))).then_some(id)).collect();
    Arc::new(crate::pcb::connectivity::Index::new(crate::pcb::connectivity::build_objects(doc,super::world::board_of(doc),None,Some(&included))))
}
pub(super) fn cached_copper(doc:&Document)->Arc<crate::pcb::connectivity::Index>{
    let cached=ACTIVE.with(|active|active.borrow().as_ref().filter(|context|context.source==doc as *const Document as usize).and_then(|context|context.cache.copper.clone()));
    if let Some(cached)=cached{return cached;}
    let index=copper_index(doc);
    ACTIVE.with(|active|{if let Some(context)=active.borrow_mut().as_mut().filter(|context|context.source==doc as *const Document as usize){context.cache.copper=Some(index.clone());}});
    index
}
pub(super) fn cached_topology(doc:&Document)->Option<Arc<Topology>>{
    ACTIVE.with(|active|active.borrow().as_ref().filter(|context|context.source==doc as *const Document as usize).map(|context|context.topology.clone()))
}
pub(super) fn cached_index(doc:&Document,layer:LayerId,ignore:&[ItemId])->Option<WorldIndex>{
    ACTIVE.with(|active|{
        let mut active=active.borrow_mut();let context=active.as_mut()?;
        if context.source!=doc as *const Document as usize{return None;}
        let key=index_key(layer,ignore);
        let index=context.cache.entries.iter().position(|(k,_)|*k==key)?;
        let entry=context.cache.entries.remove(index)?;let result=entry.1.clone();
        context.cache.entries.push_back(entry);context.cache.hits+=1;Some(result)
    })
}
pub(super) fn remember_index(doc:&Document,layer:LayerId,ignore:&[ItemId],index:WorldIndex){
    ACTIVE.with(|active|{
        let mut active=active.borrow_mut();let Some(context)=active.as_mut()else{return};
        if context.source!=doc as *const Document as usize{return;}
        context.cache.builds+=1;context.cache.entries.push_back((index_key(layer,ignore),index));
        if context.cache.entries.len()>INDEX_LIMIT{context.cache.entries.pop_front();}
    });
}
/// Restores any outer scope even if a solve unwinds. Pointer identity is only a
/// scope tag; it is never dereferenced and cannot outlive this borrowed snapshot.
struct Scope<'a>{previous:Option<Context>,cache:&'a mut IndexCache,_work:WorkScope}
impl<'a> Scope<'a>{
    fn enter(doc:&Document,cache:&'a mut IndexCache,topology:Arc<Topology>)->Self{
        let context=Context{source:doc as *const Document as usize,cache:std::mem::take(cache),topology};
        let previous=ACTIVE.with(|active|active.replace(Some(context)));Self{previous,cache,_work:WorkScope::enter()}
    }
}
impl Drop for Scope<'_>{fn drop(&mut self){
    let current=ACTIVE.with(|active|active.replace(self.previous.take()));
    if let Some(current)=current{*self.cache=current.cache;}
}}
// Speculative document snapshots need their own cache identities. The enclosing
// routing work allowance is shared, and its source cache is restored afterward.
pub(crate) fn with_snapshot<T>(doc:&Document,topology:Arc<Topology>,solve:impl FnOnce(&Topology)->T)->T{
 let mut indexes=IndexCache::default();let scope=Scope::enter(doc,&mut indexes,topology.clone());let result=solve(&topology);drop(scope);result
}
#[derive(Clone)]
enum Preview {Single(RouteResult),Pair((RouteResult,RouteResult)),Drag(Result<DragResult,String>),Parts(DragResult),Transition(Result<PairViaPlan,String>)}
#[derive(Clone,Copy,Debug,Default,Serialize)]
#[serde(rename_all="camelCase")]
pub struct EngineStats {pub index_hits:u64,pub index_builds:u64,pub preview_hits:u64,pub retained_previews:usize}

/// One immutable source board with bounded spatial and target history. No
/// candidate ever mutates the source or carries a failed shove into a later solve.
pub struct RoutingEngine {
    document:Document,
    topology:Arc<Topology>,
    indexes:IndexCache,
    previews:VecDeque<(String,Preview)>,
    preview_hits:u64,
}
impl RoutingEngine {
    pub fn new(document:Document)->Self{
        let topology=Arc::new(Topology::new(&document));
        Self{document,topology,indexes:IndexCache::default(),previews:VecDeque::new(),preview_hits:0}
    }
    pub fn matches(&self,document:&Document)->bool{self.document==*document}
    pub fn document(&self)->&Document{&self.document}
    pub fn stats(&self)->EngineStats{EngineStats{index_hits:self.indexes.hits,index_builds:self.indexes.builds,preview_hits:self.preview_hits,retained_previews:self.previews.len()}}
    fn recall(&mut self,key:&str)->Option<Preview>{
        let index=self.previews.iter().position(|(k,_)|k==key)?;
        let entry=self.previews.remove(index)?;let result=entry.1.clone();self.previews.push_back(entry);self.preview_hits+=1;Some(result)
    }
    fn retain(&mut self,key:String,preview:Preview){
        self.previews.push_back((key,preview));if self.previews.len()>PREVIEW_LIMIT{self.previews.pop_front();}
    }
    pub fn route(&mut self,req:&RouteRequest)->RouteResult{
        let key=format!("single:{req:?}");if let Some(Preview::Single(plan))=self.recall(&key){return plan;}
        let scope=Scope::enter(&self.document,&mut self.indexes,self.topology.clone());
        let plan=super::route(&self.document,req);drop(scope);
        self.retain(key,Preview::Single(plan.clone()));plan
    }
    pub fn pair(&mut self,req:&RouteRequest,negative:&str,gap:i64,start:Option<Point>)->(RouteResult,RouteResult){
        let key=format!("pair:{req:?}/{negative:?}/{gap}/{start:?}");if let Some(Preview::Pair(plan))=self.recall(&key){return plan;}
        let scope=Scope::enter(&self.document,&mut self.indexes,self.topology.clone());
        let plan=super::route_diff_pair_from(&self.document,req,negative,gap,start);drop(scope);
        self.retain(key,Preview::Pair(plan.clone()));plan
    }
    pub fn drag(&mut self,req:&TrackRequest)->Result<DragResult,String>{
        let key=format!("drag:{req:?}");if let Some(Preview::Drag(plan))=self.recall(&key){return plan;}
        let scope=Scope::enter(&self.document,&mut self.indexes,self.topology.clone());
        let plan=elastic::drag_tracks_indexed(&self.document,req,&self.topology);drop(scope);
        self.retain(key,Preview::Drag(plan.clone()));plan
    }
    pub fn parts(&mut self,req:&DragRequest)->DragResult{
        let key=format!("parts:{req:?}");if let Some(Preview::Parts(plan))=self.recall(&key){return plan;}
        let scope=Scope::enter(&self.document,&mut self.indexes,self.topology.clone());
        let plan=drag::drag_footprints(&self.document,req);drop(scope);
        self.retain(key,Preview::Parts(plan.clone()));plan
    }
    pub fn transition(&mut self,req:&PairViaRequest)->Result<PairViaPlan,String>{
        let key=format!("transition:{req:?}");if let Some(Preview::Transition(plan))=self.recall(&key){return plan;}
        let scope=Scope::enter(&self.document,&mut self.indexes,self.topology.clone());
        let plan=super::pair_via_plan(&self.document,req);drop(scope);
        self.retain(key,Preview::Transition(plan.clone()));plan
    }
}
#[cfg(test)]
mod tests{
    use super::*;
    use crate::{DocKind,Item,Command,Session,units::mm};
    use super::super::{RouteMode,CornerStyle};
    fn p(x:f64,y:f64)->Point{Point::new(mm(x),mm(y))}
    fn fixture()->Document{Document::from_item_list(DocKind::Pcb,[Item::Board(Default::default()),Item::Rect(crate::item::Rect{a:p(-10.,-10.),b:p(20.,20.),width:100_000,layer:crate::layer::BOARD_OUTLINE,fill:crate::item::FillStyle::None})])}
    fn request(end:Point)->RouteRequest{RouteRequest{net:"A".into(),layer:LayerId(0),start:p(0.,0.),end,width:250_000,mode:RouteMode::Walkaround,corner:CornerStyle::Diagonal45,ignore:vec![],posture_flip:false}}
    fn encoded(plan:&RouteResult)->String{serde_json::to_string(plan).unwrap()}
    #[test]
    fn speculative_snapshot_indexes_are_isolated_and_share_the_outer_work_allowance(){
        let doc=fixture();let child=Document::from_item_list(DocKind::Pcb,doc.items().map(|(_,item)|item.clone()).chain(std::iter::once(Item::Track(crate::pcb::Track{a:p(2.,-2.),b:p(2.,2.),width:250_000,layer:LayerId(0),net:"BLOCK".into(),locked:true}))));
        let mut indexes=IndexCache::default();let parent=Scope::enter(&doc,&mut indexes,Arc::new(Topology::new(&doc)));
        let clear=|source:&Document|{let mut world=super::super::world::World::build(source,LayerId(0),&[]);let net=world.net_id("A");let excluded=[net];world.seg_clear(super::super::geom::V::of(p(0.,0.)),super::super::geom::V::of(p(4.,0.)),&super::super::world::Probe{hw:125_000.,net,excl:&excluded})};
        assert!(clear(&doc));with_snapshot(&child,Arc::new(Topology::new(&child)),|_|{assert!(!clear(&child));assert!(!clear(&child));assert!(consume_work(5));});assert!(clear(&doc));assert!(consume_work(SEARCH_WORK-5));assert!(!consume_work(1));drop(parent);assert_eq!(indexes.builds,1);assert!(indexes.hits>=1);
    }
    #[test]
    fn failed_candidate_work_leaves_the_remaining_operation_allowance_for_alternatives(){
        let _scope=WorkScope::enter();
        {let _candidate=WorkSlice::enter(400);assert!(consume_work(399));assert!(!consume_work(2));assert!(work_exhausted());}
        assert!(!work_exhausted());assert!(consume_work(SEARCH_WORK-400));assert!(!consume_work(1));
    }
    #[test]
    fn nested_candidate_allowances_return_only_unused_work(){
        let _scope=WorkScope::enter();
        {let _outer=WorkSlice::enter(400);assert!(consume_work(100));{let _inner=WorkSlice::enter(50);assert!(consume_work(30));}assert!(consume_work(270));assert!(!consume_work(1));}
        assert!(consume_work(SEARCH_WORK-400));assert!(!consume_work(1));
    }
    #[test]
    fn shared_indexes_and_revisited_targets_keep_exact_geometry_and_source(){
        let doc=fixture();let before=crate::io::document_to_string(&doc).unwrap();let mut engine=RoutingEngine::new(doc.clone());
        let first=engine.route(&request(p(5.,1.)));
        for end in [p(6.,1.),p(7.,2.),p(8.,2.)]{
            let req=request(end);assert_eq!(encoded(&engine.route(&req)),encoded(&super::super::route(&doc,&req)));
        }
        assert!(engine.stats().index_hits>=3);assert_eq!(engine.stats().index_builds,1);
        assert_eq!(encoded(&engine.route(&request(p(5.,1.)))),encoded(&first));assert_eq!(engine.stats().preview_hits,1);
        assert_eq!(crate::io::document_to_string(engine.document()).unwrap(),before);
    }
    #[test]
    fn a_failed_target_has_no_overlay_to_contaminate_the_next_preview(){
        let doc=fixture();let mut engine=RoutingEngine::new(doc.clone());let invalid=request(p(100.,100.));
        assert!(!engine.route(&invalid).reached);
        let valid=request(p(5.,1.));let a=engine.route(&valid);assert!(a.reached);
        assert_eq!(encoded(&a),encoded(&super::super::route(&doc,&valid)));
        assert_eq!(encoded(&engine.route(&valid)),encoded(&a));
    }
    #[test]
    fn edits_and_undo_are_detected_without_changing_history(){
        let doc=fixture();let mut session=Session::with_document(doc.clone());let engine=RoutingEngine::new(doc.clone());
        session.execute(Command::Add{items:vec![Item::Track(crate::pcb::Track{a:p(2.,-2.),b:p(2.,2.),width:250_000,layer:LayerId(0),net:"B".into(),locked:false})]}).unwrap();
        assert!(!engine.matches(session.document()));session.undo().unwrap();
        assert!(engine.document().items().eq(session.document().items()));
        let fresh=RoutingEngine::new(session.document().clone());assert!(fresh.matches(session.document()));
    }
}