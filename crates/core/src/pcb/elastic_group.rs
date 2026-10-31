//! Atomic grouped conductor moves and endpoint-preserving circular-arc edits.
use super::*;
use super::super::{items::ArcTrack,query::{Obj,ObjKind,RuleSet}};

fn shifted(p:Point,d:Point)->Point{Point::new(p.x+d.x,p.y+d.y)}
fn ends(item:&Item)->Option<(Track,[Point;2])>{
    match item {
        Item::Track(t)=>Some((t.clone(),[t.a,t.b])),
        Item::ArcTrack(a)=>{let at=|deg:f64|{let r=deg.to_radians();Point::new(a.center.x+(a.radius as f64*r.cos()).round()as i64,a.center.y+(a.radius as f64*r.sin()).round()as i64)};let p=[at(a.start_angle),at(a.start_angle+a.sweep)];Some((Track{a:p[0],b:p[1],width:a.width,layer:a.layer,net:a.net.clone(),locked:a.locked},p))},
        _=>None,
    }
}
fn translated(item:&Item,d:Point)->Item{match item{Item::Track(t)=>Item::Track(Track{a:shifted(t.a,d),b:shifted(t.b,d),..t.clone()}),Item::ArcTrack(a)=>Item::ArcTrack(ArcTrack{center:shifted(a.center,d),..a.clone()}),_=>unreachable!()}}

/// Couple only explicitly configured nets and a geometrically overlapping run.
/// Fanouts without a nearby parallel partner continue to use the ordinary solver.
fn coupled(doc:&Document,id:ItemId)->Result<Option<ItemId>,String>{
    let Some(item)=doc.get(id)else{return Err("The selected conductor no longer exists".into())};
    let Some((t,_))=ends(item)else{return Err("Select tracks or circular arcs to drag".into())};
    let fallback=BoardSettings::default();let board=board_settings(doc).map_or(&fallback,|(_,b)|b);
    let Some(pair)=board.diff_pairs.iter().find(|p|p.positive==t.net||p.negative==t.net)else{return Ok(None)};
    let partner=if pair.positive==t.net{&pair.negative}else{&pair.positive};
    let (_,gap)=RuleSet::new(board).diff_pair(&Obj::new(ObjKind::Track,&t.net,Some(t.layer)));
    let mut matches=Vec::new();
    for (other,geometry) in doc.items(){
        let Some((u,_))=ends(geometry)else{continue};if u.net!=*partner||u.layer!=t.layer||u.width!=t.width{continue;}
        let pitch=(t.width+u.width)as f64/2.+gap as f64;
        let score=match (item,geometry){
            (Item::Track(a),Item::Track(b))=>{
                let dx=(a.b.x-a.a.x)as f64;let dy=(a.b.y-a.a.y)as f64;let len=dx.hypot(dy);if len<1.{continue;}
                let ex=(b.b.x-b.a.x)as f64;let ey=(b.b.y-b.a.y)as f64;let other_len=ex.hypot(ey);if other_len<1.||(dx*ey-dy*ex).abs()>len*other_len*1e-7{continue;}
                let normal=|p:Point|((p.x-a.a.x)as f64*dy-(p.y-a.a.y)as f64*dx).abs()/len;
                if (normal(b.a)-pitch).abs()>3_000.||(normal(b.b)-pitch).abs()>3_000.{continue;}
                let project=|p:Point|((p.x-a.a.x)as f64*dx+(p.y-a.a.y)as f64*dy)/len;
                let x=project(b.a);let y=project(b.b);let overlap=len.min(x.max(y))-0f64.max(x.min(y));
                if overlap<(len.min(other_len)*0.15).max(1.){continue;}overlap
            },
            (Item::ArcTrack(a),Item::ArcTrack(b))=>{
                if distance(a.center,b.center)>2.||((a.radius-b.radius).abs()as f64-pitch).abs()>3_000.||(a.sweep-b.sweep).abs()>1e-5||(a.start_angle-b.start_angle).rem_euclid(360.).min((b.start_angle-a.start_angle).rem_euclid(360.))>1e-5{continue;}a.radius as f64*a.sweep.to_radians().abs()
            },
            _=>continue,
        };
        matches.push((score,other,u.locked));
    }
    matches.sort_by(|a,b|b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
    let Some((score,other,locked))=matches.first().copied()else{return Ok(None)};
    if matches.get(1).is_some_and(|m|(m.0-score).abs()<1.){return Err("Select an unambiguous coupled pair section".into());}
    if locked{return Err("Unlock both conductors before dragging a coupled pair".into());}Ok(Some(other))
}

pub(super) fn dispatch(doc:&Document,req:&TrackRequest,topo:&Topology)->Option<Result<DragResult,String>>{
    if req.ids.is_empty()||req.ids.len()>64{return Some(Err("Select between one and 64 conductors to drag".into()));}
    let mut ids=req.ids.clone();ids.sort();ids.dedup();if ids.len()!=req.ids.len(){return Some(Err("Duplicate conductor in the selection".into()));}
    let original=ids.clone();
    for id in &original{match coupled(doc,*id){Ok(Some(other))=>{if !ids.contains(&other){ids.push(other);}},Ok(None)=>{},Err(e)=>return Some(Err(e))}}
    ids.sort();
    let has_pair=ids!=original||ids.iter().filter_map(|id|doc.get(*id).and_then(ends)).map(|(t,_)|t.net).collect::<HashSet<_>>().len()>1;
    if ids.len()==1&&matches!(doc.get(ids[0]),Some(Item::ArcTrack(_))){return Some(reshape_arc(doc,req,topo));}
    let ordinary=ids.len()<=2&&ids.iter().all(|id|matches!(doc.get(*id),Some(Item::Track(_))))&&!has_pair;
    if ordinary && (ids.len()==1 || ids.len()==2 && ends(doc.get(ids[0]).unwrap()).is_some_and(|(_,a)|ends(doc.get(ids[1]).unwrap()).is_some_and(|(_,b)|a.iter().any(|p|b.contains(p))))){return None;}
    if let Some(result)=paired_arcs(doc,req,topo,&ids){return Some(result);}
    if let Some(result)=paired(doc,req,topo,&ids){return Some(result);}
    Some(grouped(doc,req,topo,&ids))
}

fn reshape_arc(doc:&Document,req:&TrackRequest,topo:&Topology)->Result<DragResult,String>{
    let id=req.ids[0];let Some(Item::ArcTrack(a))=doc.get(id)else{unreachable!()};
    if a.locked{return Err("Unlock this curve before dragging it".into());}
    if req.delta==Point::default(){return Ok(DragResult::default());}
    if !a.start_angle.is_finite()||!a.sweep.is_finite()||a.radius<=0||a.sweep.abs()>359.999{return Err("Select a partial circular arc to reshape".into());}
    let (_,points)=ends(&Item::ArcTrack(a.clone())).unwrap();let [start,end]=points;
    let mut grab=super::super::drag::nearest_on_arc(a,req.point);
    // Short curves may consist almost entirely of their round endpoint caps.
    // Use the angular midpoint as their stable body handle rather than making
    // every possible pickup invalid because it lies within one track width.
    if distance(grab,start)<a.width as f64||distance(grab,end)<a.width as f64 {
        let angle=(a.start_angle+a.sweep/2.).to_radians();grab=Point::new(a.center.x+(a.radius as f64*angle.cos()).round()as i64,a.center.y+(a.radius as f64*angle.sin()).round()as i64);
    }
    let moved=shifted(grab,req.delta);
    if let Some(plan)=tangent_arc(doc,req,topo,a,points,moved){return plan;}
    // Work in a local frame to avoid cancellation on large board coordinates.
    let bx=(end.x-start.x)as f64;let by=(end.y-start.y)as f64;let cx=(moved.x-start.x)as f64;let cy=(moved.y-start.y)as f64;
    let det=2.*(bx*cy-by*cx);let chord=bx.hypot(by);
    let replacement=if det.abs()/chord.max(1.)<2. {
        let projection=(bx*cx+by*cy)/(chord*chord);if !(0. ..=1.).contains(&projection){return Err("That curve would fold beyond its fixed endpoints".into());}
        Item::Track(Track{a:start,b:end,width:a.width,layer:a.layer,net:a.net.clone(),locked:false})
    }else{
        let b2=bx*bx+by*by;let c2=cx*cx+cy*cy;
        let ux=(cy*b2-by*c2)/det;let uy=(bx*c2-cx*b2)/det;
        let center=Point::new(start.x+ux.round()as i64,start.y+uy.round()as i64);let radius=ux.hypot(uy).round()as i64;
        if radius<a.width/2||radius>10_000_000_000{return Err("That move cannot form a stable circular curve".into());}
        let angle=|p:Point|((p.y-center.y)as f64).atan2((p.x-center.x)as f64).to_degrees();
        let start_angle=angle(start);let ccw=(angle(end)-start_angle).rem_euclid(360.);let through=(angle(moved)-start_angle).rem_euclid(360.);
        let sweep=if through<=ccw{ccw}else{ccw-360.};
        if sweep.abs()>300.{return Err("That move would fold the curve into a loop".into());}
        let result=Item::ArcTrack(ArcTrack{center,radius,start_angle,sweep,..a.clone()});
        let (_,actual)=ends(&result).unwrap();if actual.iter().zip(points).any(|(a,b)|distance(*a,b)>2.){return Err("The curve cannot retain its fixed endpoints at this precision".into());}result
    };
    router::plan_track_edit_removed(doc,vec![IdItem{id,item:replacement}],vec![],vec![],req.mode)
}

struct Boundary{from:Point,to:Point,prototype:Track}
fn grouped(doc:&Document,req:&TrackRequest,topo:&Topology,ids:&[ItemId])->Result<DragResult,String>{
    let selected:HashSet<_>=ids.iter().copied().collect();let mut replace=Vec::new();let mut nodes:HashMap<(Point,String,LayerId),Vec<(ItemId,Track)>>=HashMap::new();
    for id in ids{
        let item=doc.get(*id).ok_or("The selected conductor no longer exists")?;let (t,points)=ends(item).ok_or("Select tracks or curves to drag")?;
        if t.locked{return Err("Unlock every selected conductor before dragging the group".into());}
        if !topo.interior_joints(*id).is_empty(){return Err("Select separate arms beside the fixed interior junction before moving a group".into());}
        for p in points{nodes.entry((p,t.net.clone(),t.layer)).or_default().push((*id,t.clone()));}
        replace.push(IdItem{id:*id,item:translated(item,req.delta)});
    }
    if req.delta==Point::default(){return Ok(DragResult::default());}
    let mut boundaries=Vec::new();let mut deleted=HashSet::new();let mut ordered:Vec<_>=nodes.into_iter().collect();ordered.sort_by(|a,b|(a.0.2,a.0.1.as_str(),a.0.0.x,a.0.0.y).cmp(&(b.0.2,b.0.1.as_str(),b.0.0.x,b.0.0.y)));
    for ((at,_,_),members) in ordered {
        let t=&members[0].1;
        let outside=topo.adj.get(&at).into_iter().flatten().any(|id|!selected.contains(id));
        let anchored=topo.anchor(at,t,&selected);
        if !outside&&!anchored{continue;}
        if members.len()>1{return Err("This group includes a fixed junction; select the conductor arms on one side".into());}
        let chain=topo.walk(at,t,&selected);
        if chain.edges.iter().any(|id|deleted.contains(id)){return Err("Select the connected loop as a whole before dragging it".into());}
        deleted.extend(chain.edges.iter().copied());
        boundaries.push(Boundary{from:chain.end,to:shifted(at,req.delta),prototype:t.clone()});
    }
    if boundaries.len()>16{return Err("Move a smaller group with at most 16 fixed boundary connections".into());}
    let mut variants:Vec<Vec<Item>>=vec![Vec::new()];
    for boundary in &boundaries {
        let mut next=Vec::new();for existing in &variants {for path in joins(boundary.from,boundary.to){
            let mut add=existing.clone();add.extend(path.windows(2).map(|w|Item::Track(Track{a:w[0],b:w[1],..boundary.prototype.clone()})));next.push(add);
        }}
        next.sort_by(|a,b|{let len=|v:&Vec<Item>|v.iter().filter_map(ends).map(|(_,p)|distance(p[0],p[1])).sum::<f64>();len(a).total_cmp(&len(b))});next.truncate(32);variants=next;
    }
    let mut last="No checked connected group reaches this position".to_string();let mut delete:Vec<_>=deleted.into_iter().collect();delete.sort();
    for add in variants {
        let copper:Vec<_>=replace.iter().map(|r|r.item.clone()).chain(add.iter().cloned()).collect();
        if !simple_conductors(&copper){continue;}
        match router::plan_track_edit_removed(doc,replace.clone(),add,delete.clone(),req.mode){Ok(plan)=>return Ok(plan),Err(e)=>last=e}
    }
    Err(last)
}

/// Reject folded joins and accidental self intersections before obstacle solving.
fn simple_conductors(items:&[Item])->bool{
    let mut edges:Vec<(Track,Point,Point)>=Vec::new();
    for item in items {match item {
        Item::Track(t)=>edges.push((t.clone(),t.a,t.b)),
        Item::ArcTrack(a)=>{let count=(a.sweep.abs()/3.).ceil().max(1.)as usize;if count>120{return false;}
            let at=|i:usize|{let angle=(a.start_angle+a.sweep*i as f64/count as f64).to_radians();Point::new(a.center.x+(a.radius as f64*angle.cos()).round()as i64,a.center.y+(a.radius as f64*angle.sin()).round()as i64)};
            let Some((t,_))=ends(item)else{return false};for i in 0..count{edges.push((t.clone(),at(i),at(i+1)));}
        },_=>return false,
    }}
    if edges.len()>1024{return false;}
    for (i,(a,p,q)) in edges.iter().enumerate(){if p==q{return false;}
        for (b,r,s) in &edges[i+1..]{if a.net!=b.net||a.layer!=b.layer{continue;}
            let close=|a:Point,b:Point|distance(a,b)<=2.;
            let shared=if close(*p,*r){Some((*p,*q,*s))}else if close(*p,*s){Some((*p,*q,*r))}else if close(*q,*r){Some((*q,*p,*s))}else if close(*q,*s){Some((*q,*p,*r))}else{None};
            if let Some((at,left,right))=shared{
                let dot=(left.x-at.x)as i128*(right.x-at.x)as i128+(left.y-at.y)as i128*(right.y-at.y)as i128;
                if dot>0{return false;}
            }else if intersects(*p,*q,*r,*s){return false;}
        }
    }true
}
// Keep a concrete collision diagnostic when a later search window only reports
// that no corridor was found. Continue searching all legal candidates.
fn remember_blocker(last:&mut Option<String>,next:String){
 let detailed=|message:&str|message.starts_with("blocked by a ")||message.starts_with("Track drag blocked:")||message.starts_with("Routing search limit reached while displacing")||message.starts_with("No clear corridor is available for connected track");
 if last.as_ref().is_none_or(|old|detailed(&next)||!detailed(old)){*last=Some(next);}
}
/// Reconstruct two offset conductors from one guide, including their rebuilt bends.
/// Independent walkarounds are deliberately not accepted as coupled geometry.
fn paired(doc:&Document,req:&TrackRequest,topo:&Topology,ids:&[ItemId])->Option<Result<DragResult,String>>{
    if ids.len()!=2||!matches!((doc.get(ids[0]),doc.get(ids[1])),(Some(Item::Track(_)),Some(Item::Track(_))))||coupled(doc,ids[0]).ok().flatten()!=Some(ids[1]){return None;}
    let mut last=None;
    // Prefer the selected boundaries, then local aligned splits before distant
    // fanout reconstruction can spend the allowance on unrelated geometry.
    match paired_window(doc,req,topo,ids,0){Some(Ok(plan))=>return Some(Ok(plan)),Some(Err(e))=>remember_blocker(&mut last,e),None=>return None,}
    // Consume an adjacent bend before splitting another local body. This keeps
    // repeated drags from ratcheting a valid paired section into tiny stubs.
    {let _window_work=router::WorkSlice::enter(1000);match paired_window(doc,req,topo,ids,1){Some(Ok(plan))=>return Some(Ok(plan)),Some(Err(e))=>remember_blocker(&mut last,e),None=>{}}}
    match paired_local(doc,req,ids){Some(Ok(plan))=>return Some(Ok(plan)),Some(Err(e))=>remember_blocker(&mut last,e),None=>{}}
    for limit in [2,4,usize::MAX]{match paired_window(doc,req,topo,ids,limit){Some(Ok(plan))=>return Some(Ok(plan)),Some(Err(e))=>remember_blocker(&mut last,e),None=>{}}}
    last.map(Err)
}
fn paired_window(doc:&Document,req:&TrackRequest,topo:&Topology,ids:&[ItemId],limit:usize)->Option<Result<DragResult,String>>{
    if ids.len()!=2{return None;}
    let (Some(Item::Track(p)),Some(Item::Track(q)))=(doc.get(ids[0]),doc.get(ids[1]))else{return None};
    if coupled(doc,ids[0]).ok().flatten()!=Some(ids[1]){return None;}
    if req.delta==Point::default(){return Some(Ok(DragResult::default()));}
    if !topo.interior_joints(ids[0]).is_empty()||!topo.interior_joints(ids[1]).is_empty(){return Some(Err("Drag a coupled section away from the fixed branch".into()));}
    let dot=(p.b.x-p.a.x)as i128*(q.b.x-q.a.x)as i128+(p.b.y-p.a.y)as i128*(q.b.y-q.a.y)as i128;
    let (qa,qb)=if dot>=0{(q.a,q.b)}else{(q.b,q.a)};
    let selected=ids.iter().copied().collect();
    let (pl,ptail_left)=window_chain(doc,p.a,topo.walk(p.a,p,&selected),limit);let (pr,ptail_right)=window_chain(doc,p.b,topo.walk(p.b,p,&selected),limit);
    let (ql,qtail_left)=window_chain(doc,qa,topo.walk(qa,q,&selected),limit);let (qr,qtail_right)=window_chain(doc,qb,topo.walk(qb,q,&selected),limit);
    let connected=|at:Point,t:&Track|topo.anchor(at,t,&selected)||topo.adj.get(&at).into_iter().flatten().any(|id|!selected.contains(id));
    if [&pl,&pr,&ql,&qr].iter().all(|c|c.edges.is_empty())&&![p.a,p.b].iter().any(|at|connected(*at,p))&&![qa,qb].iter().any(|at|connected(*at,q)){return None;}
    let midpoint=|a:Point,b:Point|Point::new((a.x+b.x)/2,(a.y+b.y)/2);
    let start=midpoint(pl.end,ql.end);let end=midpoint(pr.end,qr.end);
    let dx=(p.b.x-p.a.x)as f64;let dy=(p.b.y-p.a.y)as f64;let len=dx.hypot(dy);let direction=(dx/len,dy/len);
    let sep=(qa.x-p.a.x)as f64*(-direction.1)+(qa.y-p.a.y)as f64*direction.0;
    let pitch=sep.abs();let normal_shift=req.delta.x as f64*(-direction.1)+req.delta.y as f64*direction.0;
    let delta=Point::new((-direction.1*normal_shift).round()as i64,(direction.0*normal_shift).round()as i64);
    let left=shifted(midpoint(p.a,qa),delta);let right=shifted(midpoint(p.b,qb),delta);
    let tangent=|c:&Chain,t:&Track,at:Point,fallback:(f64,f64)|{
        let far=c.edges.last().and_then(|id|if let Some(Item::Track(t))=doc.get(*id){Some(if t.a==c.end{t.b}else{t.a})}else{None});
        let v=far.unwrap_or_else(||shifted(at,Point::new((fallback.0*1_000_000.).round()as i64,(fallback.1*1_000_000.).round()as i64)));
        let x=(v.x-c.end.x)as f64;let y=(v.y-c.end.y)as f64;let n=x.hypot(y);let _=t;if n<1.{fallback}else{(x/n,y/n)}
    };
    let first=tangent(&pl,p,pl.end,direction);let last=tangent(&pr,p,pr.end,(-direction.0,-direction.1));
    let mut delete:Vec<_>=[&pl,&pr,&ql,&qr].into_iter().flat_map(|c|c.edges.iter().copied()).collect();delete.sort();delete.dedup();
    // An offset guide is exact only across runs with matching end headings.
    // Anchors with other spacing receive checked fanout joins below.
    let offset=|path:&[Point],amount:f64|->Vec<Point>{
        let normals:Vec<_>=path.windows(2).map(|w|{let x=(w[1].x-w[0].x)as f64;let y=(w[1].y-w[0].y)as f64;let len=x.hypot(y);(-y/len,x/len)}).collect();
        (0..path.len()).map(|i|{let (x,y)=if i==0{(normals[0].0*amount,normals[0].1*amount)}else if i+1==path.len(){(normals[i-1].0*amount,normals[i-1].1*amount)}else{let n=normals[i-1];let m=normals[i];let den=1.+n.0*m.0+n.1*m.1;((n.0+m.0)*amount/den,(n.1+m.1)*amount/den)};Point::new(path[i].x+x.round()as i64,path[i].y+y.round()as i64)}).collect()
    };
    let fix=|mut path:Vec<Point>,a:Point,b:Point|->Vec<Vec<Point>>{
        // Integer offset rounding can miss an existing endpoint by a few nm.
        // Snap it exactly instead of adding a microscopic reversing lead.
        if distance(path[0],a)<=4.{path[0]=a;}
        if distance(*path.last().unwrap(),b)<=4.{*path.last_mut().unwrap()=b;}
        let mut out=Vec::new();for prefix in joins(a,path[0]){for suffix in joins(*path.last().unwrap(),b){let mut p=prefix.clone();p.extend(path.iter().skip(1));p.extend(suffix.iter().skip(1));out.push(simplify(&p));}}out
    };
    let mut error=None;
    let mut guides=Vec::new();
    for amount in [0.,pitch*0.25,pitch*0.5,pitch,2.*pitch,4.*pitch]{
        let lead=|at:Point,d:(f64,f64)|{let distance=distance(start,end).min(len)/3.;let distance=distance.min(amount);shifted(at,Point::new((d.0*distance).round()as i64,(d.1*distance).round()as i64))};
        let a=lead(start,first);let b=lead(end,last);
        for x in joins(a,left){for y in joins(right,b){
            let mut center=vec![start];center.extend(x.iter().copied());center.push(right);center.extend(y.into_iter().skip(1));center.push(end);guides.push(simplify(&center));
        }}
        // Slide a supporting rail instead of tethering its old endpoints to
        // translated points. The two offset conductors consume bends together.
        let normal=(-direction.1,direction.0);let plane=normal.0*left.x as f64+normal.1*left.y as f64;
        let hits=|at:Point,forward:f64|{
            let mut out=Vec::new();for (x,y) in DIRS {
                if (x as f64*direction.0+y as f64*direction.1)*forward < -1e-7{continue;}
                let den=normal.0*x as f64+normal.1*y as f64;if den.abs()<1e-8{continue;}
                let distance=(plane-normal.0*at.x as f64-normal.1*at.y as f64)/den;
                if distance>=-1.{out.push(Point::new((at.x as f64+x as f64*distance).round()as i64,(at.y as f64+y as f64*distance).round()as i64));}
            }out
        };
        for x in hits(a,1.){for y in hits(b,-1.){if (y.x-x.x)as f64*direction.0+(y.y-x.y)as f64*direction.1<=0.{continue;}guides.push(simplify(&[start,a,x,y,b,end]));}}
    }
    guides.sort_by(|a,b|{let length=|p:&Vec<Point>|p.windows(2).map(|w|distance(w[0],w[1])).sum::<f64>();length(a).total_cmp(&length(b)).then_with(||a.iter().map(|p|(p.x,p.y)).cmp(b.iter().map(|p|(p.x,p.y))))});guides.dedup();
    for center in guides.into_iter().take(48){
            if center.len()<2||!simple_path(&center){continue;}
            let pp=offset(&center,-sep/2.);let np=offset(&center,sep/2.);
            for positive in fix(pp,pl.end,pr.end){for negative in fix(np.clone(),ql.end,qr.end){
                let whole=|left:&Vec<Point>,path:&Vec<Point>,right:&Vec<Point>|simplify(&left.iter().rev().chain(path.iter().skip(1)).chain(right.iter().skip(1)).copied().collect::<Vec<_>>());
                if !simple_path(&whole(&ptail_left,&positive,&ptail_right))||!simple_path(&whole(&qtail_left,&negative,&qtail_right)){continue;}
                let make=|path:&[Point],t:&Track|path.windows(2).map(|w|Item::Track(Track{a:w[0],b:w[1],..t.clone()})).collect::<Vec<_>>();
                let mut pt=make(&positive,p);let mut nt=make(&negative,q);if pt.is_empty()||nt.is_empty(){continue;}
                let choose=|items:&mut Vec<Item>,target:Point|{let index=items.iter().enumerate().min_by(|(_,a),(_,b)|{let gap=|item:&Item|{let (_,ends)=ends(item).unwrap();super::super::connectivity::geo::point_seg(super::super::connectivity::pt(target),super::super::connectivity::pt(ends[0]),super::super::connectivity::pt(ends[1])).0};gap(a).total_cmp(&gap(b))}).map_or(0,|(i,_)|i);items.remove(index)};
                let replace=vec![IdItem{id:ids[0],item:choose(&mut pt,shifted(req.point,delta))},IdItem{id:ids[1],item:choose(&mut nt,shifted(req.point,delta))}];pt.extend(nt);
                let copper:Vec<_>=replace.iter().map(|r|r.item.clone()).chain(pt.iter().cloned()).collect();if !simple_conductors(&copper){continue;}
                let _candidate_work=router::WorkSlice::enter(1000);
                match router::plan_track_edit_removed(doc,replace,pt,delete.clone(),req.mode){Ok(plan)=>return Some(Ok(plan)),Err(e)=>remember_blocker(&mut error,e)}
            }}
    }
    Some(Err(error.unwrap_or_else(||"No room to keep this pair coupled at that position".into())))
}
#[cfg(test)]
#[path="elastic_group_tests.rs"]
mod tests;

/// Retain existing straight tangents while moving the circular body. A fixed
/// pad, via, branch or locked neighbor fixes that endpoint instead of sliding it.
fn tangent_arc(doc:&Document,req:&TrackRequest,topo:&Topology,a:&ArcTrack,points:[Point;2],moved:Point)->Option<Result<DragResult,String>>{
    type Vec2=(f64,f64);
    let add=|a:Vec2,b:Vec2|(a.0+b.0,a.1+b.1);let sub=|a:Vec2,b:Vec2|(a.0-b.0,a.1-b.1);let mul=|a:Vec2,k:f64|(a.0*k,a.1*k);let dot=|a:Vec2,b:Vec2|a.0*b.0+a.1*b.1;let cross=|a:Vec2,b:Vec2|a.0*b.1-a.1*b.0;
    let of=|p:Point|(p.x as f64,p.y as f64);let round=|v:Vec2|Point::new(v.0.round()as i64,v.1.round()as i64);
    let arc_id=req.ids[0];let excluded=HashSet::from([arc_id]);let prototype=ends(&Item::ArcTrack(a.clone())).unwrap().0;
    let locked_tangent=std::cell::Cell::new(false);
    let neighbors:Vec<_>=points.iter().map(|at|{
        if topo.anchor(*at,&prototype,&excluded){return None;}
        let candidates:Vec<_>=topo.tracks.iter().filter(|(_,t)|t.net==a.net&&t.layer==a.layer&&(distance(t.a,*at)<=2.||distance(t.b,*at)<=2.)).collect();
        if candidates.len()!=1{return None;}let (id,t)=candidates[0];if !topo.interior_joints(*id).is_empty(){return None;}
        let far=if distance(t.a,*at)<=2.{t.b}else{t.a};let v=sub(of(far),of(*at));let len=dot(v,v).sqrt();let radial=sub(of(*at),of(a.center));
        if len<1.||dot(v,radial).abs()>len*a.radius as f64*1e-5{return None;}
        if t.locked{locked_tangent.set(true);return None;}
        let direction=mul(v,1./len);let normal=(-direction.1,direction.0);Some((*id,t.clone(),far,direction,normal))
    }).collect();
    if locked_tangent.get(){return Some(Err("A locked straight tangent fixes this curve; unlock it before reshaping".into()));}
    if neighbors.iter().all(Option::is_none){return None;}
    let quadratic=|aa:f64,bb:f64,cc:f64|->Vec<f64>{if aa.abs()<1e-12{if bb.abs()<1e-10{vec![]}else{vec![-cc/bb]}}else{let discriminant=bb*bb-4.*aa*cc;if discriminant<0.{vec![]}else{let root=discriminant.sqrt();vec![(-bb+root)/(2.*aa),(-bb-root)/(2.*aa)]}}};
    let mut circles:Vec<(Vec2,f64)>=Vec::new();let cursor=of(moved);
    if let (Some(left),Some(right))=(&neighbors[0],&neighbors[1]){
        let (d0,n0)=(left.3,left.4);let d1=right.3;let den=cross(d0,d1);
        if den.abs()>1e-8 {
            let origin=add(of(points[0]),mul(d0,cross(sub(of(points[1]),of(points[0])),d1)/den));
            let old=sub(of(a.center),origin);let size=dot(old,old).sqrt();if size<1.{return Some(Err("The curve has no stable tangent intersection".into()));}
            let axis=mul(old,1./size);let k=dot(axis,n0).abs();let v=sub(cursor,origin);
            for parameter in quadratic(1.-k*k,-2.*dot(axis,v),dot(v,v)){if parameter>0.{circles.push((add(origin,mul(axis,parameter)),parameter*k));}}
        }else{
            let separation=dot(sub(of(points[1]),of(points[0])),n0);let radius=separation.abs()/2.;
            let base=add(of(points[0]),mul(n0,separation/2.));let v=sub(cursor,base);let along=dot(v,d0);let perpendicular=dot(v,n0);let determinant=radius*radius-perpendicular*perpendicular;
            if determinant>=0.{for parameter in [along+determinant.sqrt(),along-determinant.sqrt()]{circles.push((add(base,mul(d0,parameter)),radius));}}
        }
    }else{
        let tangent=if neighbors[0].is_some(){0}else{1};let neighbor=neighbors[tangent].as_ref().unwrap();let fixed=of(points[1-tangent]);let chord=sub(cursor,fixed);let length=dot(chord,chord).sqrt();if length<1.{return Some(Err("Move the curve body away from its fixed endpoint".into()));}
        let axis=(-chord.1/length,chord.0/length);let base=mul(add(cursor,fixed),0.5);let v=sub(base,cursor);let signed=dot(sub(base,of(points[tangent])),neighbor.4);let projection=dot(axis,neighbor.4);
        for parameter in quadratic(projection*projection-1.,2.*(signed*projection-dot(axis,v)),signed*signed-dot(v,v)){
            let center=add(base,mul(axis,parameter));let radius=dot(sub(center,cursor),sub(center,cursor)).sqrt();circles.push((center,radius));
        }
    }
    // Prefer the nearby solution, retaining the source sweep orientation.
    circles.sort_by(|x,y|{let distance=|p:Vec2|{let v=sub(p,of(a.center));dot(v,v)};distance(x.0).total_cmp(&distance(y.0))});
    let mut last="No legal tangent-preserving curve reaches this position".to_string();
    for (center,radius) in circles {
        if !center.0.is_finite()||!center.1.is_finite()||radius<a.width as f64/2.||radius>10_000_000_000.{continue;}
        let center=round(center);let radius=radius.round()as i64;let mut projected=points;
        for i in 0..2 {if let Some(neighbor)=&neighbors[i]{let point=sub(of(center),mul(neighbor.4,dot(sub(of(center),of(points[i])),neighbor.4)));projected[i]=round(point);}}
        let angle=|p:Point|((p.y-center.y)as f64).atan2((p.x-center.x)as f64).to_degrees();let start=angle(projected[0]);let sweep=if a.sweep>=0.{(angle(projected[1])-start).rem_euclid(360.)}else{-(start-angle(projected[1])).rem_euclid(360.)};
        let through=if sweep>=0.{(angle(moved)-start).rem_euclid(360.)}else{(start-angle(moved)).rem_euclid(360.)};if sweep.abs()<1e-6||sweep.abs()>300.||through>sweep.abs()+1e-6{continue;}
        let curve=Item::ArcTrack(ArcTrack{center,radius,start_angle:start,sweep,..a.clone()});let (_,actual)=ends(&curve).unwrap();let mut replace=vec![IdItem{id:arc_id,item:curve}];let mut valid=true;
        for i in 0..2 {if let Some((id,t,far,_,_))=&neighbors[i]{let old=sub(of(points[i]),of(*far));let new=sub(of(actual[i]),of(*far));if dot(old,new)<=0.{valid=false;break;}let mut t=t.clone();if distance(t.a,points[i])<=2.{t.a=actual[i]}else{t.b=actual[i]};replace.push(IdItem{id:*id,item:Item::Track(t)});}else if distance(actual[i],points[i])>2.{valid=false;break;}}
        if !valid||!simple_conductors(&replace.iter().map(|r|r.item.clone()).collect::<Vec<_>>()){continue;}
        match router::plan_track_edit_removed(doc,replace,vec![],vec![],req.mode){Ok(plan)=>return Some(Ok(plan)),Err(e)=>last=e}
    }
    Some(Err(last))
}
/// Deform concentric coupled curves jointly, retaining the two tangent rails
/// at each end. Fixed lands and branching copper remain hard constraints.
fn paired_arcs(doc:&Document,req:&TrackRequest,topo:&Topology,ids:&[ItemId])->Option<Result<DragResult,String>>{
    if ids.len()!=2{return None;}
    let (Some(Item::ArcTrack(primary)),Some(Item::ArcTrack(mate)))=(doc.get(ids[0]),doc.get(ids[1]))else{return None};
    if coupled(doc,ids[0]).ok().flatten()!=Some(ids[1]){return None;}
    let (_,ends_p)=ends(&Item::ArcTrack(primary.clone())).unwrap();let (prototype,ends_m)=ends(&Item::ArcTrack(mate.clone())).unwrap();
    let selected:HashSet<_>=ids.iter().copied().collect();
    let attached=|at:Point,net:&str|topo.tracks.values().any(|t|t.layer==primary.layer&&t.net==net&&(distance(t.a,at)<=2.||distance(t.b,at)<=2.));
    if !ends_p.iter().any(|p|attached(*p,&primary.net))&&!ends_m.iter().any(|p|attached(*p,&mate.net)){return None;}
    Some((||{
        if primary.locked||mate.locked{return Err("Unlock both curves before reshaping the pair".into());}
        if req.delta==Point::default(){return Ok(DragResult::default());}
        let mut neighbors=Vec::new();
        for at in ends_m{
            let candidates:Vec<_>=topo.tracks.iter().filter(|(_,t)|t.net==mate.net&&t.layer==mate.layer&&(distance(t.a,at)<=2.||distance(t.b,at)<=2.)).collect();
            if candidates.len()>1{return Err("A fixed branch prevents this paired curve deformation".into());}
            if let Some((id,t))=candidates.first(){
                if t.locked||!topo.interior_joints(**id).is_empty(){return Err("A fixed tangent prevents this paired curve deformation".into());}
                let far=if distance(t.a,at)<=2.{t.b}else{t.a};let v=((far.x-at.x)as f64,(far.y-at.y)as f64);let radial=((at.x-mate.center.x)as f64,(at.y-mate.center.y)as f64);
                if (v.0*radial.0+v.1*radial.1).abs()>v.0.hypot(v.1)*mate.radius as f64*1e-5{return Err("The partner curve needs a straight tangent connection".into());}
                neighbors.push(Some((**id,(*t).clone(),far)));
            }else{neighbors.push(None);}
        }
        let mut work=doc.clone();let mut ignored=vec![ids[1]];ignored.extend(neighbors.iter().flatten().map(|(id,_,_)|*id));
        work.apply(&ignored.iter().map(|id|crate::document::Edit{id:*id,before:work.get(*id).cloned(),after:None}).collect::<Vec<_>>());
        let request=TrackRequest{ids:vec![ids[0]],..req.clone()};let mut plan=reshape_arc(&work,&request,&Topology::new(&work))?;
        let Some(Item::ArcTrack(curve))=plan.replace.iter().find(|r|r.id==ids[0]).map(|r|&r.item)else{return Err("This pair cannot flatten while retaining its coupled curve spacing".into())};
        let radius=curve.radius+mate.radius-primary.radius;if radius<=mate.width/2{return Err("The inner partner would collapse; choose a larger curve".into());}
        let changed=Item::ArcTrack(ArcTrack{center:curve.center,radius,start_angle:curve.start_angle,sweep:curve.sweep,..mate.clone()});let (_,points)=ends(&changed).unwrap();
        for (i,at) in points.iter().enumerate(){
            if topo.anchor(ends_m[i],&prototype,&selected)&&distance(*at,ends_m[i])>2.{return Err("The partner curve endpoint is fixed to a pad or via".into());}
            if let Some((id,t,far))=&neighbors[i]{
                let old=((ends_m[i].x-far.x)as i128,(ends_m[i].y-far.y)as i128);let new=((at.x-far.x)as i128,(at.y-far.y)as i128);if old.0*new.0+old.1*new.1<=0{return Err("The partner tangent would reverse direction".into());}
                let cross=(t.b.x-t.a.x)as i128*(at.y-t.a.y)as i128-(t.b.y-t.a.y)as i128*(at.x-t.a.x)as i128;let length=distance(t.a,t.b);if cross.abs()as f64>2.*length{return Err("This move cannot retain the partner tangent".into());}
                let mut t=t.clone();if distance(t.a,ends_m[i])<=2.{t.a=*at}else{t.b=*at};plan.replace.push(IdItem{id:*id,item:Item::Track(t)});
            }
        }
        plan.replace.push(IdItem{id:ids[1],item:changed});
        let copper:Vec<_>=plan.replace.iter().map(|r|r.item.clone()).chain(plan.add.iter().cloned()).collect();if !simple_conductors(&copper){return Err("That paired curve would fold its attached routing".into());}
        router::plan_track_edit_removed(doc,plan.replace,plan.add,plan.delete,req.mode)
    })())
}
/// Match local gates on staggered parallel segments. Virtual splits stay in
/// the candidate; final edits are validated against the original identities.
fn paired_local(doc:&Document,req:&TrackRequest,ids:&[ItemId])->Option<Result<DragResult,String>>{
    let mut last=None;
    for divisor in [3.,4.,6.,8.,12.]{match paired_local_window(doc,req,ids,divisor){Some(Ok(plan))=>return Some(Ok(plan)),Some(Err(e))=>remember_blocker(&mut last,e),None=>{}}}
    last.map(Err)
}
fn paired_local_window(doc:&Document,req:&TrackRequest,ids:&[ItemId],divisor:f64)->Option<Result<DragResult,String>>{
    if ids.len()!=2{return None;}let (Some(Item::Track(p)),Some(Item::Track(q)))=(doc.get(ids[0]),doc.get(ids[1]))else{return None};
    let dx=(p.b.x-p.a.x)as f64;let dy=(p.b.y-p.a.y)as f64;let length=dx.hypot(dy);if length<1.{return None;}
    let direction=(dx/length,dy/length);let along=|point:Point|(point.x-p.a.x)as f64*direction.0+(point.y-p.a.y)as f64*direction.1;
    let qa=along(q.a);let qb=along(q.b);let lo=0f64.max(qa.min(qb));let hi=length.min(qa.max(qb));let pitch=(((q.a.x-p.a.x)as f64*(-direction.1)+(q.a.y-p.a.y)as f64*direction.0)).abs();
    // Gate margins depend on the offset corner footprint and the requested
    // displacement, not an arbitrary number of full pair pitches.
    let normal_shift=req.delta.x as f64*(-direction.1)+req.delta.y as f64*direction.0;
    let margin=(pitch/4.).max(2.);let needed=pitch*0.5+2.*normal_shift.abs()+8.;
    if hi-lo<needed+2.*margin{return None;}
    let target=along(req.point).clamp(lo+margin+needed/2.,hi-margin-needed/2.);let extent=(hi-lo).min(4_000_000.)/divisor;
    let low=(target-extent).max(lo+margin);let high=(target+extent).min(hi-margin);if high-low<needed{return None;}
    let mut work=doc.clone();let mut splits=Vec::new();
    for (id,t) in [(ids[0],p),(ids[1],q)]{
        let point=|projection:f64|{let delta=projection-along(t.a);Point::new(t.a.x+(direction.0*delta).round()as i64,t.a.y+(direction.1*delta).round()as i64)};
        let (a,b)=if along(t.a)<along(t.b){(point(low),point(high))}else{(point(high),point(low))};
        splits.push(crate::document::Edit{id,before:doc.get(id).cloned(),after:Some(Item::Track(Track{a,b,..t.clone()}))});
        for (a,b) in [(t.a,a),(b,t.b)]{if a!=b{splits.push(crate::document::Edit{id:work.alloc_id(),before:None,after:Some(Item::Track(Track{a,b,..t.clone()}))});}}
    }
    work.apply(&splits);let topology=std::sync::Arc::new(Topology::new(&work));let result=router::with_snapshot(&work,topology,|topology|paired_window(&work,req,topology,ids,0));let plan=match result{Some(Ok(plan))=>plan,Some(Err(error))=>return Some(Err(error)),None=>return None};let rerouted=plan.rerouted;
    let mut edits:Vec<_>=plan.replace.into_iter().map(|r|crate::document::Edit{id:r.id,before:work.get(r.id).cloned(),after:Some(r.item)}).collect();edits.extend(plan.delete.into_iter().map(|id|crate::document::Edit{id,before:work.get(id).cloned(),after:None}));work.apply(&edits);
    let adds:Vec<_>=plan.add.into_iter().map(|item|crate::document::Edit{id:work.alloc_id(),before:None,after:Some(item)}).collect();work.apply(&adds);
    let mut replace=Vec::new();let mut delete=Vec::new();let mut add=Vec::new();for(id,item)in doc.items(){match work.get(id){Some(after)if after!=item=>replace.push(IdItem{id,item:after.clone()}),None=>delete.push(id),_=>{}}}for(id,item)in work.items(){if doc.get(id).is_none(){add.push(item.clone());}}
    Some(router::plan_track_edit_removed(doc,replace,add,delete,req.mode).map(|mut plan|{plan.rerouted+=rerouted;plan}))
}