//! Analytic circular copper distances, independent of display tessellation.
use super::{P,Bb,Gap,PolyGeo,sub,dot,dist,mid,point_seg};
#[derive(Clone,Copy,Debug)]
pub struct CircularArc{pub center:P,pub radius:f64,pub start:f64,pub sweep:f64,pub half_width:f64}
impl CircularArc{
    pub fn point(&self,angle:f64)->P{let a=angle.to_radians();[self.center[0]+self.radius*a.cos(),self.center[1]+self.radius*a.sin()]}
    fn angle_contains(&self,angle:f64)->bool{self.sweep.abs()>=360.-1e-9||if self.sweep>=0.{(angle-self.start).rem_euclid(360.)<=self.sweep+1e-9}else{(self.start-angle).rem_euclid(360.)<=-self.sweep+1e-9}}
    fn contains(&self,p:P)->bool{self.angle_contains((p[1]-self.center[1]).atan2(p[0]-self.center[0]).to_degrees())}
    fn endpoints(&self)->[P;2]{[self.point(self.start),self.point(self.start+self.sweep)]}
    pub fn bounds(&self)->Bb{let [a,b]=self.endpoints();let mut bb=Bb::of(a,b);for angle in [0.,90.,180.,270.]{if self.angle_contains(angle){bb.add(self.point(angle));}}bb.grow(self.half_width)}
    fn nearest(&self,p:P)->P{let [a,b]=self.endpoints();let mut near=if dist(p,a)<=dist(p,b){a}else{b};let radial=sub(p,self.center);let length=dot(radial,radial).sqrt();if length>1e-12{let candidate=[self.center[0]+radial[0]*self.radius/length,self.center[1]+radial[1]*self.radius/length];if self.contains(candidate)&&dist(p,candidate)<dist(p,near){near=candidate;}}near}
    fn segment_distance(&self,a:P,b:P)->(f64,P,P){
        let [start,end]=self.endpoints();let mut candidates=vec![(start,point_seg(start,a,b).1),(end,point_seg(end,a,b).1),(self.nearest(a),a),(self.nearest(b),b)];
        let v=sub(b,a);let length2=dot(v,v);if length2>1e-12{
            let offset=sub(a,self.center);let along=-dot(offset,v)/length2;let foot=[a[0]+v[0]*along,a[1]+v[1]*along];let perpendicular=dist(foot,self.center);
            if (0. ..=1.).contains(&along)&&perpendicular>1e-12{let radial=sub(foot,self.center);for sign in [-1.,1.]{let at=[self.center[0]+sign*radial[0]*self.radius/perpendicular,self.center[1]+sign*radial[1]*self.radius/perpendicular];if self.contains(at){candidates.push((at,foot));}}}
            let discriminant=self.radius*self.radius-perpendicular*perpendicular;
            if discriminant>=-1e-6{let distance=(discriminant.max(0.)/length2).sqrt();for t in [along-distance,along+distance]{if (0. ..=1.).contains(&t){let at=[a[0]+v[0]*t,a[1]+v[1]*t];if self.contains(at){return (0.,at,at);}}}}
        }
        candidates.into_iter().map(|(p,q)|(dist(p,q),p,q)).min_by(|a,b|a.0.total_cmp(&b.0)).unwrap()
    }
}
pub fn arc_cap(arc:&CircularArc,a:P,b:P,r:f64,limit:f64)->Option<Gap>{let (d,p,q)=arc.segment_distance(a,b);let gap=d-arc.half_width-r;(gap<=limit).then(||(gap,mid(p,q)))}
pub fn arc_arc(a:&CircularArc,b:&CircularArc,limit:f64)->Option<Gap>{
    let mut candidates=Vec::new();for p in a.endpoints(){candidates.push((p,b.nearest(p)));}for q in b.endpoints(){candidates.push((a.nearest(q),q));}
    let v=sub(b.center,a.center);let distance=dot(v,v).sqrt();
    if distance>1e-12{
        let unit=[v[0]/distance,v[1]/distance];for sign in [-1.,1.]{let p=[a.center[0]+sign*unit[0]*a.radius,a.center[1]+sign*unit[1]*a.radius];if !a.contains(p){continue;}for other in [-1.,1.]{let q=[b.center[0]+other*unit[0]*b.radius,b.center[1]+other*unit[1]*b.radius];if b.contains(q){candidates.push((p,q));}}}
        let along=(a.radius*a.radius-b.radius*b.radius+distance*distance)/(2.*distance);let height2=a.radius*a.radius-along*along;
        if height2>=-1e-6&&distance<=a.radius+b.radius+1e-6&&distance+1e-6>=(a.radius-b.radius).abs(){let base=[a.center[0]+unit[0]*along,a.center[1]+unit[1]*along];let height=height2.max(0.).sqrt();for sign in [-1.,1.]{let p=[base[0]-sign*unit[1]*height,base[1]+sign*unit[0]*height];if a.contains(p)&&b.contains(p){let gap=-a.half_width-b.half_width;return (gap<=limit).then_some((gap,p));}}}
    }
    let (distance,p,q)=candidates.into_iter().map(|(p,q)|(dist(p,q),p,q)).min_by(|a,b|a.0.total_cmp(&b.0)).unwrap();let gap=distance-a.half_width-b.half_width;(gap<=limit).then(||(gap,mid(p,q)))
}
pub fn arc_poly(arc:&CircularArc,poly:&PolyGeo,limit:f64)->Option<Gap>{
    let start=arc.point(arc.start);if poly.contains(start){return Some((-arc.half_width-1.,start));}
    let mut best:Option<Gap>=None;poly.edges_near(&arc.bounds().grow(limit.max(0.)),|a,b|{let reach=best.map_or(limit,|x|x.0.min(limit));if let Some(gap)=arc_cap(arc,a,b,0.,reach){if best.is_none_or(|x|gap.0<x.0){best=Some(gap);}}});best
}
#[cfg(test)]
mod tests{
 use super::*;
 fn arc(radius:f64,start:f64,sweep:f64)->CircularArc{CircularArc{center:[0.,0.],radius,start,sweep,half_width:125_000.}}
 #[test]fn concentric_exact_gap_does_not_depend_on_curve_segmentation(){for sweep in [45.,90.,-180.,270.,360.]{let a=arc(2_000_000.,17.,sweep);let b=arc(2_450_000.,17.,sweep);assert!((arc_arc(&a,&b,200_001.).unwrap().0-200_000.).abs()<1e-6);assert!(arc_arc(&a,&b,199_999.).is_none());}}
 #[test]fn arcs_test_true_intersections_and_sector_endpoints(){let a=arc(2_000_000.,0.,90.);assert!(arc_cap(&a,[-3_000_000.,0.],[3_000_000.,0.],0.,0.).unwrap().0<0.);assert!(arc_cap(&a,[-3_000_000.,-1_000_000.],[-3_000_000.,1_000_000.],0.,0.).is_none());let mut b=arc(2_000_000.,90.,180.);b.center=[2_000_000.,0.];assert!(arc_arc(&a,&b,0.).unwrap().0<0.);}
 #[test]fn arc_caps_and_polygon_holes_use_the_same_physical_copper(){let a=arc(2_000_000.,0.,360.);let poly=PolyGeo::new(vec![vec![[-4e6,-4e6],[4e6,-4e6],[4e6,4e6],[-4e6,4e6]],vec![[-3e6,-3e6],[3e6,-3e6],[3e6,3e6],[-3e6,3e6]]]);assert!(arc_poly(&a,&poly,500_000.).is_none());assert!(arc_poly(&a,&poly,1_000_000.).is_some());let at=a.point(45.);assert!((arc_cap(&a,at,at,0.,0.).unwrap().0+125_000.).abs()<1e-6);}
}