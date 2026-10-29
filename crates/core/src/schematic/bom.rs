//! Bill of materials export.

use std::collections::BTreeSet;

use super::{Compiled, natural_cmp};

struct Group<'a> {
    component: &'a str,
    value: &'a str,
    footprint: &'a str,
    library: &'a str,
    description: &'a str,
    designators: Vec<&'a str>,
    params: Vec<(&'a str, &'a str)>,
}

/// CSV with columns `Designator,Quantity,Value,Footprint,Component,Library,Description`
/// followed by every extra parameter name (sorted). Parts with identical
/// component, value and footprint share a row.
pub fn bom_csv(compiled: &Compiled) -> String {
    let extra: BTreeSet<&str> = compiled.parts.iter().filter(|p| !p.excluded_from_bom()).flat_map(|p| p.parameters.iter().map(|(n, _)| n.as_str())).collect();

    let mut groups: Vec<Group> = Vec::new();
    for p in compiled.parts.iter().filter(|p| !p.excluded_from_bom()) {
        let g = match groups.iter_mut().position(|g| g.component == p.component && g.value == p.value && g.footprint == p.footprint) {
            Some(i) => &mut groups[i],
            None => {
                groups.push(Group {
                    component: &p.component,
                    value: &p.value,
                    footprint: &p.footprint,
                    library: &p.library,
                    description: &p.description,
                    designators: Vec::new(),
                    params: Vec::new(),
                });
                groups.last_mut().unwrap()
            }
        };
        g.designators.push(&p.designator);
        if g.description.is_empty() {
            g.description = &p.description;
        }
        for (n, v) in &p.parameters {
            if !v.is_empty() && !g.params.iter().any(|(gn, _)| gn == n) {
                g.params.push((n, v));
            }
        }
    }
    for g in &mut groups {
        g.designators.sort_by(|a, b| natural_cmp(a, b));
    }
    groups.sort_by(|a, b| natural_cmp(a.designators[0], b.designators[0]));

    let mut out = String::new();
    let header: Vec<&str> = ["Designator", "Quantity", "Value", "Footprint", "Component", "Library", "Description"]
        .into_iter()
        .chain(extra.iter().copied())
        .collect();
    push_row(&mut out, header.iter().map(|s| s.to_string()));
    for g in &groups {
        let fixed = [
            g.designators.join(", "),
            g.designators.len().to_string(),
            g.value.to_string(),
            g.footprint.to_string(),
            g.component.to_string(),
            g.library.to_string(),
            g.description.to_string(),
        ];
        let params = extra.iter().map(|n| g.params.iter().find(|(gn, _)| gn == n).map_or(String::new(), |(_, v)| v.to_string()));
        push_row(&mut out, fixed.into_iter().chain(params));
    }
    out
}

fn push_row(out: &mut String, cells: impl Iterator<Item = String>) {
    let cells: Vec<String> = cells.map(|c| csv_cell(&c)).collect();
    out.push_str(&cells.join(","));
    out.push_str("\r\n");
}

fn csv_cell(s: &str) -> String {
    let needs = s.contains([',', '"', '\n', '\r']) || s.starts_with(' ') || s.ends_with(' ');
    if needs { format!("\"{}\"", s.replace('"', "\"\"")) } else { s.to_string() }
}
