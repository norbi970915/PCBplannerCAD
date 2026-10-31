//! Bounded routing snapshots, isolated from the workspace lock during solving.
use std::{collections::VecDeque,sync::{Arc,Mutex}};
use pcbplannercad_core::{Document,pcb::router::RoutingEngine};
type SharedEngine=Arc<Mutex<Option<RoutingEngine>>>;
#[derive(Default)]
pub struct RoutingState(Mutex<VecDeque<(String,SharedEngine)>>);
impl RoutingState {
    pub fn solve<R>(&self,id:&str,document:Document,solve:impl FnOnce(&mut RoutingEngine)->R)->Result<R,String>{
        let shared={
            let mut engines=self.0.lock().map_err(|_|"Routing state is unavailable; reopen the board".to_string())?;
            let position=engines.iter().position(|(key,_)|key==id);
            let entry=position.and_then(|index|engines.remove(index)).unwrap_or_else(||(id.into(),Arc::new(Mutex::new(None))));
            let shared=entry.1.clone();engines.push_back(entry);
            while engines.len()>3{engines.pop_front();}
            shared
        };
        let mut engine=shared.lock().map_err(|_|"Routing snapshot is unavailable; reopen the board".to_string())?;
        if !engine.as_ref().is_some_and(|engine|engine.matches(&document)){*engine=Some(RoutingEngine::new(document));}
        Ok(solve(engine.as_mut().unwrap()))
    }
}
#[cfg(test)]
mod tests{
    use super::*;
    use pcbplannercad_core::{DocKind,Item,Point,LayerId,pcb::router::{RouteRequest,RouteMode,CornerStyle}};
    #[test]
    fn geometry_changes_replace_the_snapshot_and_never_reuse_an_old_target(){
        let state=RoutingState::default();let doc=Document::from_item_list(DocKind::Pcb,[Item::Board(Default::default())]);
        let req=RouteRequest{net:"A".into(),layer:LayerId(0),start:Point::new(0,0),end:Point::new(5_000_000,0),width:250_000,mode:RouteMode::Walkaround,corner:CornerStyle::Diagonal45,ignore:vec![],posture_flip:false};
        state.solve("board",doc.clone(),|engine|engine.route(&req)).unwrap();
        let reused=state.solve("board",doc.clone(),|engine|{engine.route(&req);engine.stats()}).unwrap();assert_eq!(reused.preview_hits,1);
        let changed=Document::from_item_list(DocKind::Pcb,doc.item_list().into_iter().chain([Item::Track(pcbplannercad_core::pcb::Track{a:Point::new(2_000_000,-1_000_000),b:Point::new(2_000_000,1_000_000),width:250_000,layer:LayerId(0),net:"B".into(),locked:true})]));
        let fresh=state.solve("board",changed.clone(),|engine|{assert!(engine.matches(&changed));engine.route(&req);engine.stats()}).unwrap();assert_eq!(fresh.preview_hits,0);
        assert_eq!(state.solve("other",doc,|engine|engine.stats().retained_previews).unwrap(),0);
    }
}