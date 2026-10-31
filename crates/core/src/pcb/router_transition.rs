//! Joint, read-only planning for paired layer transitions.
use super::*;
use crate::document::Edit;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all="camelCase")]
pub struct PairViaRequest {
    pub positive: Via,
    pub negative: Via,
    pub source: LayerId,
    pub target: LayerId,
    pub direction: Point,
    pub width: Coord,
    pub corner: CornerStyle,
    pub mode: RouteMode,
    pub tracks: Vec<Track>,
    pub moved: Vec<IdItem>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all="camelCase")]
pub struct PairViaPlan {pub tracks:Vec<Track>,pub vias:Vec<Via>}

fn blocker(world:&mut World,layer:&str,a:Point,b:Point,width:Coord,net:&str)->Option<String>{
    let key=world.net_id(net);let excluded=[key];let probe=Probe{hw:width as f64/2.0,net:key,excl:&excluded};
    if world.seg_clear(V::of(a),V::of(b),&probe){return None;}
    let detail=world.seg_collisions(V::of(a),V::of(b),&probe).first().map(|hit|match hit{
        world::Hit::Ob(index)=>{let item=&world.obstacles[*index as usize];format!("{}{} on net '{}'",item.kind.name(),item.id.map_or(String::new(),|id|format!(" #{}",id.0)),world.net_name(item.net))},
        _=>world.hit_kind(*hit).name().to_string(),
    }).unwrap_or_else(||"board edge or required clearance".into());
    Some(format!("Pair transition blocked on {layer} by {detail} (physical via span checked)"))
}

pub fn pair_via_plan(doc:&Document,req:&PairViaRequest)->Result<PairViaPlan,String>{
    let clearance=pair_via_clearance(doc,&req.positive,&req.negative,req.source)?;
    let board=world::board_of(doc);
    if req.width<=0 || req.source==req.target {return Err("Choose a different copper layer and positive track width".into());}
    for via in [&req.positive,&req.negative] {
        if via.drill<=0||via.diameter<=via.drill||!board.stackup.span(via.from_layer,via.to_layer).contains(&req.target){return Err("Both pair vias must connect the source and target layers with valid annuli".into());}
    }
    let separation=V::of(req.negative.pos)-V::of(req.positive.pos);
    if separation.len()<1.0{return Err("Pair endpoints coincide; choose a clear routing position".into());}
    let side=separation.norm();let mut forward=V::new(-side.y,side.x);
    if forward.dot(V::of(req.direction))<0.0{forward=forward*-1.0;}
    let required=(req.positive.diameter+req.negative.diameter)as f64/2.0+clearance as f64+16.0;
    let spread=((required-separation.len())/2.0).max(0.0);
    let lead=spread.max(req.positive.diameter.max(req.negative.diameter)as f64/2.0+req.width as f64+clearance as f64);
    let lateral=required.max(separation.len());
    let mut offsets=Vec::new();
    if spread==0.0{offsets.push((0.0,0.0));}
    for scale in [1.0,1.8,2.6]{for shift in [0.0,-lateral,lateral]{offsets.push((lead*scale,shift));}}
    // The immutable base contains preview shove edits but no stale pour copper.
    let mut base=doc.clone();
    let removed:std::collections::HashSet<_>=req.moved.iter().map(|m|m.id).collect();
    let edits:Vec<_>=base.items().filter_map(|(id,item)|(removed.contains(&id)||matches!(item,Item::Zone(z)if !z.keepout)).then(||Edit{id,before:Some(item.clone()),after:None})).collect();base.apply(&edits);
    let mut worlds:Vec<_>=board.stackup.copper_layers().into_iter().map(|layer|(layer,World::build(&base,layer,&[]))).collect();
    let mut first_failure=None;
    for (ahead,shift) in offsets {
        for flip in [false,true] {
            let mut vias=vec![req.positive.clone(),req.negative.clone()];
            let mut tracks=req.tracks.clone();
            for (i,via) in vias.iter_mut().enumerate(){
                let original=via.pos;
                via.pos=(V::of(original)+forward*ahead+side*(shift+if i==0{-spread}else{spread})).round();
                tracks.extend(tracks_of(&corner_path(original,via.pos,req.corner,flip),req.width,req.source,&via.net));
            }
            if req.mode==RouteMode::Ignore{return Ok(PairViaPlan{tracks,vias});}
            // Cheap indexed rejection precedes the complete, all-layer DRC gate.
            let mut collision=None;
            for (layer,world) in &mut worlds{
                let name=board.stackup.layers.iter().find(|l|l.layer==Some(*layer)).map_or("copper layer",|l|l.name.as_str());
                for via in vias.iter().filter(|via|board.stackup.span(via.from_layer,via.to_layer).contains(layer)){
                    collision=blocker(world,name,via.pos,via.pos,via.diameter,&via.net);if collision.is_some(){break;}
                }
                if collision.is_none(){for track in tracks.iter().filter(|t|t.layer==*layer){collision=blocker(world,name,track.a,track.b,track.width,&track.net);if collision.is_some(){break;}}}
                if collision.is_some(){break;}
            }
            if let Some(message)=collision{first_failure.get_or_insert(message);continue;}
            let mut check=base.clone();let mut touched=Vec::new();
            let edits:Vec<_>=tracks.iter().cloned().map(Item::Track).chain(vias.iter().cloned().map(Item::Via)).chain(req.moved.iter().map(|m|m.item.clone())).map(|item|{let id=check.alloc_id();touched.push(id);Edit{id,before:None,after:Some(item)}}).collect();check.apply(&edits);
            if let Some(violation)=super::super::drc::run_near(&check,&touched).into_iter().find(|v|v.severity==super::super::drc::Severity::Error){first_failure.get_or_insert_with(||format!("Pair transition blocked: {}",violation.message));continue;}
            return Ok(PairViaPlan{tracks,vias});
        }
    }
    Err(first_failure.unwrap_or_else(||"No legal paired via transition fits here".into()))
}
