//! Center-anchored pad launches with bounded, shape-aware alternatives.
use super::*;
use super::super::geometry::WorldPad;

pub(super) fn launches(pad: &WorldPad, width: Coord, corner: CornerStyle, toward: Point) -> Vec<Point> {
    let polygon = pad.polygon(0);
    let mut candidates = Vec::new();
    let target = V::of(toward) - V::of(pad.pos);
    for i in 0..8 {
        if corner == CornerStyle::Orthogonal90 && i % 2 != 0 { continue; }
        let angle = i as f64 * std::f64::consts::FRAC_PI_4;
        let dir = V::new(angle.cos(), angle.sin());
        if dir.dot(target) < 0.0 { continue; }
        let mut extent: f64 = 0.0;
        for j in 0..polygon.len() {
            let a = V::of(polygon[j]) - V::of(pad.pos);
            let edge = V::of(polygon[(j + 1) % polygon.len()]) - V::of(polygon[j]);
            let cross = |a: V, b: V| a.x * b.y - a.y * b.x;
            let den = cross(dir, edge);
            if den.abs() < 1e-9 { continue; }
            let distance = cross(a, edge) / den;
            let along = cross(a, dir) / den;
            if distance >= 0.0 && (0.0..=1.0).contains(&along) { extent = extent.max(distance); }
        }
        let reach = (extent + width as f64 / 2.0 + 4.0).ceil() as i64;
        // Integer octilinear rails, including rotated pad shapes.
        let dx = angle.cos().round() as i64;
        let dy = angle.sin().round() as i64;
        let scale = if dx != 0 && dy != 0 { (reach as f64 / std::f64::consts::SQRT_2).ceil() as i64 } else { reach };
        let point = Point::new(pad.pos.x + dx * scale, pad.pos.y + dy * scale);
        candidates.push((penalty(pad, point) + (V::of(point) - V::of(toward)).len(), point));
    }
    candidates.sort_by(|a,b| a.0.total_cmp(&b.0));
    candidates.into_iter().map(|(_,p)|p).take(5).collect()
}

fn penalty(pad: &WorldPad, next: Point) -> f64 {
    if pad.width == pad.height { return 0.0; }
    let angle = (pad.rotation + if pad.height > pad.width {90.0} else {0.0}).to_radians();
    let alignment = (V::of(next)-V::of(pad.pos)).norm().dot(V::new(angle.cos(),angle.sin())).abs();
    (1.0-alignment*alignment) * (pad.width-pad.height).abs() as f64 * 1.5
}

pub(super) fn route(world: &mut World, probe: &Probe, doc: &Document, req: &RouteRequest) -> Outcome {
    if req.start == req.end || req.corner==CornerStyle::Any { return route_in(world,probe,req.start,req.end,req.mode,req.corner,req.posture_flip); }
    let pads = engine::cached_pads(doc);
    let at = |point| pads.iter().find(|p|p.pos==point&&p.net==req.net&&p.on_layer(req.layer)).map(|p|p.for_layer(req.layer));
    let source = at(req.start); let target = at(req.end);
    let starts = source.as_ref().map_or_else(||vec![req.start],|p|launches(p,req.width,req.corner,req.end));
    let ends = target.as_ref().map_or_else(||vec![req.end],|p|launches(p,req.width,req.corner,req.start));
    let score = |path:&[Point]| {
        let length:f64=path.windows(2).map(|w|(V::of(w[1])-V::of(w[0])).len()).sum();
        length + path.len() as f64 * req.width as f64 * 0.2
            + source.as_ref().map_or(0.0,|p|path.get(1).map_or(0.0,|next|penalty(p,*next)))
            + target.as_ref().map_or(0.0,|p|path.iter().rev().nth(1).map_or(0.0,|next|penalty(p,*next)))
    };
    let mut choices = vec![(score(&corner_path(req.start,req.end,req.corner,req.posture_flip)),corner_path(req.start,req.end,req.corner,req.posture_flip))];
    for start in &starts { for end in &ends {
        for flip in [req.posture_flip,!req.posture_flip] {
            let mut path=vec![req.start];path.extend(corner_path(*start,*end,req.corner,flip));path.push(req.end);
            let path=simplify(&path);
            if super::super::elastic::simple_path(&path) { choices.push((score(&path),path)); }
        }
    }}
    choices.sort_by(|a,b|a.0.total_cmp(&b.0));
    for (_,path) in choices { if world.path_clear(&path,probe) {
        return Outcome{path,reached:true,moved:vec![],blocked_at:None,message:String::new()};
    }}
    let original=route_in(world,probe,req.start,req.end,req.mode,req.corner,req.posture_flip);
    if original.reached || source.is_none() { return original; }
    // A narrow launch can escape a pad row before the main search begins.
    for launch in starts.into_iter().take(3) {
        if engine::work_exhausted(){break;}
        world.removed.clear();world.added.clear();
        if !world.path_clear(&[req.start,launch],probe) { continue; }
        let mut result=route_in(world,probe,launch,req.end,req.mode,req.corner,req.posture_flip);
        if !result.reached { continue; }
        let mut path=vec![req.start];path.extend(result.path);path=simplify(&path);
        if super::super::elastic::simple_path(&path)&&world.path_clear(&path,probe) {result.path=path;return result;}
    }
    world.removed.clear();world.added.clear();original
}
