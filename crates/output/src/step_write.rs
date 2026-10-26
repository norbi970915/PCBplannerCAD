//! ISO-10303-21 (STEP AP214, AUTOMOTIVE_DESIGN) writer for triangle meshes.
//!
//! File structure: a root assembly product named after the export, with one
//! child product per mesh (so every mesh imports as a separate named body).
//! Each child has a `SHAPE_REPRESENTATION` (placement only) linked by a
//! `SHAPE_REPRESENTATION_RELATIONSHIP` to the geometry representation:
//! - closed (2-manifold, consistently oriented) meshes → `FACETED_BREP`
//!   (a `MANIFOLD_SOLID_BREP` subtype) of a `CLOSED_SHELL`, in a
//!   `FACETED_BREP_SHAPE_REPRESENTATION`;
//! - anything else → `SHELL_BASED_SURFACE_MODEL` of an `OPEN_SHELL`, in a
//!   `MANIFOLD_SURFACE_SHAPE_REPRESENTATION`.
//!
//! Every triangle is a `FACE_SURFACE` bounded by a `FACE_OUTER_BOUND` /
//! `POLY_LOOP` over shared `CARTESIAN_POINT`s, on a `PLANE` through its first
//! vertex. Colors are attached with `STYLED_ITEM` → `PRESENTATION_STYLE_ASSIGNMENT`
//! → `SURFACE_STYLE_USAGE` → … → `COLOUR_RGB` and collected in one
//! `MECHANICAL_DESIGN_GEOMETRIC_PRESENTATION_REPRESENTATION`. Units are mm.

use std::collections::HashMap;
use std::fmt::Write as _;

use super::Mesh;
use crate::OutputError;

struct Writer {
    out: String,
    next: u64,
}

impl Writer {
    /// Starts entity `#id=` and returns its id; the caller writes the body
    /// and calls [`Writer::end`].
    fn begin(&mut self) -> u64 {
        let id = self.next;
        self.next += 1;
        let _ = write!(self.out, "#{id}=");
        id
    }

    fn end(&mut self) {
        self.out.push_str(";\n");
    }

    fn ent(&mut self, body: std::fmt::Arguments) -> u64 {
        let id = self.begin();
        let _ = self.out.write_fmt(body);
        self.end();
        id
    }

    fn real(&mut self, v: f64) {
        push_real(&mut self.out, v);
    }

    fn refs(&mut self, ids: &[u64]) {
        self.out.push('(');
        for (i, id) in ids.iter().enumerate() {
            if i > 0 {
                self.out.push(',');
                if i % 16 == 0 {
                    self.out.push('\n');
                }
            }
            let _ = write!(self.out, "#{id}");
        }
        self.out.push(')');
    }
}

/// STEP REAL: always has a decimal point, trailing zeros trimmed (1 nm resolution in mm).
fn push_real(out: &mut String, v: f64) {
    let v = if v.is_finite() { v } else { 0.0 };
    let start = out.len();
    let _ = write!(out, "{v:.6}");
    while out.ends_with('0') {
        out.pop();
    }
    if &out[start..] == "-0." {
        out.truncate(start);
        out.push_str("0.");
    }
}

/// STEP string literal contents (without the quotes).
pub(super) fn step_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\'' => out.push_str("''"),
            '\\' => out.push_str("\\\\"),
            ' '..='~' => out.push(c),
            c if (c as u32) <= 0xFFFF => {
                let _ = write!(out, "\\X2\\{:04X}\\X0\\", c as u32);
            }
            c => {
                let _ = write!(out, "\\X4\\{:08X}\\X0\\", c as u32);
            }
        }
    }
    out
}

/// A mesh after vertex merging and degenerate-triangle removal.
struct Prepared<'a> {
    mesh: &'a Mesh,
    name: String,
    points: Vec<[f32; 3]>,
    tris: Vec<[u32; 3]>,
    normals: Vec<[f64; 3]>,
    closed: bool,
}

fn key(p: [f32; 3]) -> [u32; 3] {
    // +0.0 and -0.0 merge.
    p.map(|c| if c == 0.0 { 0 } else { c.to_bits() })
}

fn prepare(mesh: &Mesh) -> Result<Prepared<'_>, OutputError> {
    if !mesh.positions.len().is_multiple_of(3) || !mesh.indices.len().is_multiple_of(3) {
        return Err(OutputError::Invalid(format!("mesh '{}': positions/indices length not a multiple of 3", mesh.name)));
    }
    let nv = mesh.positions.len() / 3;
    let mut remap = vec![u32::MAX; nv];
    let mut points: Vec<[f32; 3]> = Vec::new();
    let mut index: HashMap<[u32; 3], u32> = HashMap::with_capacity(nv);
    let mut tris = Vec::with_capacity(mesh.indices.len() / 3);
    let mut normals = Vec::with_capacity(mesh.indices.len() / 3);
    for t in mesh.indices.chunks_exact(3) {
        let mut ids = [0u32; 3];
        let mut pos = [[0f64; 3]; 3];
        for k in 0..3 {
            let i = t[k] as usize;
            if i >= nv {
                return Err(OutputError::Invalid(format!("mesh '{}': index {i} out of range", mesh.name)));
            }
            if remap[i] == u32::MAX {
                let p = [mesh.positions[3 * i], mesh.positions[3 * i + 1], mesh.positions[3 * i + 2]];
                if p.iter().any(|c| !c.is_finite()) {
                    return Err(OutputError::Invalid(format!("mesh '{}': non-finite vertex", mesh.name)));
                }
                remap[i] = *index.entry(key(p)).or_insert_with(|| {
                    points.push(p);
                    (points.len() - 1) as u32
                });
            }
            ids[k] = remap[i];
            let p = points[ids[k] as usize];
            pos[k] = [p[0] as f64, p[1] as f64, p[2] as f64];
        }
        if ids[0] == ids[1] || ids[1] == ids[2] || ids[0] == ids[2] {
            continue;
        }
        let u = [pos[1][0] - pos[0][0], pos[1][1] - pos[0][1], pos[1][2] - pos[0][2]];
        let v = [pos[2][0] - pos[0][0], pos[2][1] - pos[0][1], pos[2][2] - pos[0][2]];
        let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
        let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        if len.is_nan() || len <= 1e-14 {
            continue;
        }
        tris.push(ids);
        normals.push([n[0] / len, n[1] / len, n[2] / len]);
    }
    // Closed iff every directed edge appears exactly once and so does its reverse.
    let mut edges: HashMap<(u32, u32), u32> = HashMap::with_capacity(tris.len() * 3);
    for t in &tris {
        for k in 0..3 {
            *edges.entry((t[k], t[(k + 1) % 3])).or_insert(0) += 1;
        }
    }
    let closed = !tris.is_empty() && edges.iter().all(|(&(a, b), &c)| c == 1 && edges.get(&(b, a)) == Some(&1));
    Ok(Prepared { mesh, name: String::new(), points, tris, normals, closed })
}

pub(super) fn write(name: &str, meshes: &[Mesh], timestamp: &str) -> Result<String, OutputError> {
    let mut prepared = Vec::new();
    let mut used_names: HashMap<String, usize> = HashMap::new();
    for (i, m) in meshes.iter().enumerate() {
        let mut p = prepare(m)?;
        if p.tris.is_empty() {
            continue;
        }
        let base = if m.name.trim().is_empty() { format!("Body{}", i + 1) } else { m.name.clone() };
        let n = used_names.entry(base.clone()).or_insert(0);
        *n += 1;
        p.name = if *n == 1 { base } else { format!("{base}_{n}") };
        prepared.push(p);
    }
    if prepared.is_empty() {
        return Err(OutputError::Invalid("nothing to export: no non-degenerate triangles".into()));
    }

    let tri_total: usize = prepared.iter().map(|p| p.tris.len()).sum();
    let pt_total: usize = prepared.iter().map(|p| p.points.len()).sum();
    let mut w = Writer { out: String::with_capacity(tri_total * 260 + pt_total * 50 + 4096), next: 1 };
    let file = step_string(name);
    let _ = write!(
        w.out,
        "ISO-10303-21;\nHEADER;\nFILE_DESCRIPTION(('PCBPlannerCAD 3D board model'),'2;1');\n\
         FILE_NAME('{file}.step','{timestamp}',(''),(''),'PCBPlannerCAD','PCBPlannerCAD','');\n\
         FILE_SCHEMA(('AUTOMOTIVE_DESIGN {{ 1 0 10303 214 1 1 1 1 }}'));\nENDSEC;\nDATA;\n"
    );

    let app = w.ent(format_args!("APPLICATION_CONTEXT('core data for automotive mechanical design processes')"));
    w.ent(format_args!("APPLICATION_PROTOCOL_DEFINITION('international standard','automotive_design',2000,#{app})"));
    let len_unit = w.ent(format_args!("(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.))"));
    let ang_unit = w.ent(format_args!("(NAMED_UNIT(*)PLANE_ANGLE_UNIT()SI_UNIT($,.RADIAN.))"));
    let sol_unit = w.ent(format_args!("(NAMED_UNIT(*)SI_UNIT($,.STERADIAN.)SOLID_ANGLE_UNIT())"));
    let unc = w.ent(format_args!("UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.E-06),#{len_unit},'distance_accuracy_value','confusion accuracy')"));
    let ctx = w.ent(format_args!(
        "(GEOMETRIC_REPRESENTATION_CONTEXT(3)GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#{unc}))GLOBAL_UNIT_ASSIGNED_CONTEXT((#{len_unit},#{ang_unit},#{sol_unit}))REPRESENTATION_CONTEXT('Context #1','3D Context with UNIT and UNCERTAINTY'))"
    ));
    let prod_ctx = w.ent(format_args!("PRODUCT_CONTEXT('',#{app},'mechanical')"));
    let pd_ctx = w.ent(format_args!("PRODUCT_DEFINITION_CONTEXT('part definition',#{app},'design')"));
    let origin = w.ent(format_args!("CARTESIAN_POINT('',(0.,0.,0.))"));
    let zdir = w.ent(format_args!("DIRECTION('',(0.,0.,1.))"));
    let xdir = w.ent(format_args!("DIRECTION('',(1.,0.,0.))"));
    let root_axis = w.ent(format_args!("AXIS2_PLACEMENT_3D('',#{origin},#{zdir},#{xdir})"));

    let mut products = Vec::new();
    let mut styled = Vec::new();
    let mut styles: HashMap<[u32; 3], u64> = HashMap::new();
    // (child SR, child PD, child axis, root axis, name)
    let mut children: Vec<(u64, u64, u64, u64, String)> = Vec::new();
    let mut normal_ids: HashMap<[i64; 3], u64> = HashMap::new();

    for p in &prepared {
        let pname = step_string(&p.name);
        // Points.
        let first_point = w.next;
        for pt in &p.points {
            w.begin();
            w.out.push_str("CARTESIAN_POINT('',(");
            w.real(pt[0] as f64);
            w.out.push(',');
            w.real(pt[1] as f64);
            w.out.push(',');
            w.real(pt[2] as f64);
            w.out.push_str("))");
            w.end();
        }
        // Faces.
        let mut faces = Vec::with_capacity(p.tris.len());
        for (t, n) in p.tris.iter().zip(&p.normals) {
            let nk = n.map(|c| (c * 1e9).round() as i64);
            let nid = match normal_ids.get(&nk) {
                Some(&id) => id,
                None => {
                    let id = w.begin();
                    w.out.push_str("DIRECTION('',(");
                    w.real(n[0]);
                    w.out.push(',');
                    w.real(n[1]);
                    w.out.push(',');
                    w.real(n[2]);
                    w.out.push_str("))");
                    w.end();
                    normal_ids.insert(nk, id);
                    id
                }
            };
            let [a, b, c] = t.map(|i| first_point + i as u64);
            let lp = w.ent(format_args!("POLY_LOOP('',(#{a},#{b},#{c}))"));
            let ax = w.ent(format_args!("AXIS2_PLACEMENT_3D('',#{a},#{nid},$)"));
            let pl = w.ent(format_args!("PLANE('',#{ax})"));
            let bd = w.ent(format_args!("FACE_OUTER_BOUND('',#{lp},.T.)"));
            faces.push(w.ent(format_args!("FACE_SURFACE('',(#{bd}),#{pl},.T.)")));
        }
        let (body, geo) = if p.closed {
            let id = w.begin();
            w.out.push_str("CLOSED_SHELL('',");
            w.refs(&faces);
            w.out.push(')');
            w.end();
            let body = w.ent(format_args!("FACETED_BREP('{pname}',#{id})"));
            let geo = w.ent(format_args!("FACETED_BREP_SHAPE_REPRESENTATION('{pname}',(#{body}),#{ctx})"));
            (body, geo)
        } else {
            let id = w.begin();
            w.out.push_str("OPEN_SHELL('',");
            w.refs(&faces);
            w.out.push(')');
            w.end();
            let body = w.ent(format_args!("SHELL_BASED_SURFACE_MODEL('{pname}',(#{id}))"));
            let geo = w.ent(format_args!("MANIFOLD_SURFACE_SHAPE_REPRESENTATION('{pname}',(#{body}),#{ctx})"));
            (body, geo)
        };
        let child_axis = w.ent(format_args!("AXIS2_PLACEMENT_3D('',#{origin},#{zdir},#{xdir})"));
        let sr = w.ent(format_args!("SHAPE_REPRESENTATION('{pname}',(#{child_axis}),#{ctx})"));
        w.ent(format_args!("SHAPE_REPRESENTATION_RELATIONSHIP('','',#{sr},#{geo})"));
        let prod = w.ent(format_args!("PRODUCT('{pname}','{pname}','',(#{prod_ctx}))"));
        products.push(prod);
        let pdf = w.ent(format_args!("PRODUCT_DEFINITION_FORMATION('','',#{prod})"));
        let pd = w.ent(format_args!("PRODUCT_DEFINITION('design','',#{pdf},#{pd_ctx})"));
        let pds = w.ent(format_args!("PRODUCT_DEFINITION_SHAPE('','',#{pd})"));
        w.ent(format_args!("SHAPE_DEFINITION_REPRESENTATION(#{pds},#{sr})"));
        let root_child_axis = w.ent(format_args!("AXIS2_PLACEMENT_3D('',#{origin},#{zdir},#{xdir})"));
        children.push((sr, pd, child_axis, root_child_axis, pname));

        // Color.
        let rgb = p.mesh.color.map(|c| if c.is_finite() { c.clamp(0.0, 1.0) } else { 0.5 });
        let psa = match styles.get(&rgb.map(f32::to_bits)) {
            Some(&id) => id,
            None => {
                let col = w.begin();
                w.out.push_str("COLOUR_RGB('',");
                w.real(rgb[0] as f64);
                w.out.push(',');
                w.real(rgb[1] as f64);
                w.out.push(',');
                w.real(rgb[2] as f64);
                w.out.push(')');
                w.end();
                let fasc = w.ent(format_args!("FILL_AREA_STYLE_COLOUR('',#{col})"));
                let fas = w.ent(format_args!("FILL_AREA_STYLE('',(#{fasc}))"));
                let ssfa = w.ent(format_args!("SURFACE_STYLE_FILL_AREA(#{fas})"));
                let sss = w.ent(format_args!("SURFACE_SIDE_STYLE('',(#{ssfa}))"));
                let ssu = w.ent(format_args!("SURFACE_STYLE_USAGE(.BOTH.,#{sss})"));
                let psa = w.ent(format_args!("PRESENTATION_STYLE_ASSIGNMENT((#{ssu}))"));
                styles.insert(rgb.map(f32::to_bits), psa);
                psa
            }
        };
        styled.push(w.ent(format_args!("STYLED_ITEM('color',(#{psa}),#{body})")));
    }

    // Root assembly.
    let root_name = step_string(if name.trim().is_empty() { "Board" } else { name });
    let mut root_items = vec![root_axis];
    root_items.extend(children.iter().map(|c| c.3));
    let root_sr = w.begin();
    let _ = write!(w.out, "SHAPE_REPRESENTATION('{root_name}',");
    w.refs(&root_items);
    let _ = write!(w.out, ",#{ctx})");
    w.end();
    let root_prod = w.ent(format_args!("PRODUCT('{root_name}','{root_name}','',(#{prod_ctx}))"));
    products.push(root_prod);
    let root_pdf = w.ent(format_args!("PRODUCT_DEFINITION_FORMATION('','',#{root_prod})"));
    let root_pd = w.ent(format_args!("PRODUCT_DEFINITION('design','',#{root_pdf},#{pd_ctx})"));
    let root_pds = w.ent(format_args!("PRODUCT_DEFINITION_SHAPE('','',#{root_pd})"));
    w.ent(format_args!("SHAPE_DEFINITION_REPRESENTATION(#{root_pds},#{root_sr})"));
    for (i, (sr, pd, child_axis, root_axis_i, pname)) in children.iter().enumerate() {
        let nauo = w.ent(format_args!("NEXT_ASSEMBLY_USAGE_OCCURRENCE('NAUO{}','{pname}','',#{root_pd},#{pd},$)", i + 1));
        let pds = w.ent(format_args!("PRODUCT_DEFINITION_SHAPE('Placement','Placement of an item',#{nauo})"));
        let idt = w.ent(format_args!("ITEM_DEFINED_TRANSFORMATION('','',#{child_axis},#{root_axis_i})"));
        let rr = w.ent(format_args!(
            "(REPRESENTATION_RELATIONSHIP('','',#{sr},#{root_sr})REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION(#{idt})SHAPE_REPRESENTATION_RELATIONSHIP())"
        ));
        w.ent(format_args!("CONTEXT_DEPENDENT_SHAPE_REPRESENTATION(#{rr},#{pds})"));
    }
    w.begin();
    w.out.push_str("PRODUCT_RELATED_PRODUCT_CATEGORY('part',$,");
    w.refs(&products);
    w.out.push(')');
    w.end();
    w.begin();
    w.out.push_str("MECHANICAL_DESIGN_GEOMETRIC_PRESENTATION_REPRESENTATION('',");
    w.refs(&styled);
    let _ = write!(w.out, ",#{ctx})");
    w.end();
    w.out.push_str("ENDSEC;\nEND-ISO-10303-21;\n");
    Ok(w.out)
}

/// UTC timestamp `YYYY-MM-DDThh:mm:ss` for the STEP header.
pub(super) fn now_timestamp() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Civil-from-days (Howard Hinnant).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}", rem / 3600, rem % 3600 / 60, rem % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reals_and_strings_are_step_literals() {
        let mut s = String::new();
        for v in [1.0, -0.0, 0.25, -1.5e-7, 12.3456789] {
            push_real(&mut s, v);
            s.push(' ');
        }
        assert_eq!(s, "1. 0. 0.25 0. 12.345679 ");
        assert_eq!(step_string("R1 'a'\\µ"), "R1 ''a''\\\\\\X2\\00B5\\X0\\");
        assert_eq!(now_timestamp().len(), 19);
    }
}
