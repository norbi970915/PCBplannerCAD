//! Scalar members of vector/group buses, kept separate from graphical bus wires.
use super::SheetRef;
use super::connectivity::{ElemKind, SheetConn, UnionFind};
use super::erc::Report;
use crate::geom::on_segment;
use crate::item::LabelScope;
use crate::{Document, Item};
use std::collections::{BTreeMap, BTreeSet};

type Aliases = BTreeMap<String, Vec<String>>;
const MAX_MEMBERS: usize = 4096;

pub(super) fn aliases(sheets: &[SheetRef], report: &mut Report) -> Aliases {
    let mut out = Aliases::new();
    for sheet in sheets {
        for (name, members) in sheet.doc.bus_aliases() {
            let members: Vec<_> = members
                .iter()
                .cloned()
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();
            if out.get(name).is_some_and(|old| old != &members) {
                report.error(
                    "bus-alias-conflict",
                    format!("Bus alias '{name}' has inconsistent definitions across sheets"),
                    &sheet.path,
                    vec![],
                    None,
                );
                out.insert(name.clone(), vec![]);
            } else {
                out.insert(name.clone(), members);
            }
        }
    }
    out
}

fn vector(text: &str) -> Option<(&str, usize, usize)> {
    let (prefix, range) = text.strip_suffix(']')?.rsplit_once('[')?;
    if prefix.is_empty() {
        return None;
    }
    let (a, b) = range.split_once("..")?;
    Some((prefix, a.parse().ok()?, b.parse().ok()?))
}

fn split_members(text: &str) -> Result<Vec<&str>, String> {
    let mut out = vec![];
    let mut start = None;
    let mut depth = 0usize;
    for (i, c) in text.char_indices() {
        if c == '"' || c == '\\' {
            return Err("quoted or escaped bus member names are not supported yet".into());
        }
        if c.is_whitespace() && depth == 0 {
            if let Some(s) = start.take() {
                out.push(&text[s..i]);
            }
        } else {
            start.get_or_insert(i);
            if c == '{' {
                depth += 1;
            }
            if c == '}' {
                depth = depth.checked_sub(1).ok_or("unbalanced bus group")?;
            }
        }
    }
    if depth != 0 {
        return Err("unbalanced bus group".into());
    }
    if let Some(s) = start {
        out.push(&text[s..]);
    }
    Ok(out)
}

fn group(text: &str) -> Option<(&str, &str)> {
    let (prefix, rest) = text.split_once('{')?;
    if prefix.ends_with(['~', '^', '_', '$']) {
        return None;
    } // Scalar text markup.
    Some((prefix, rest.strip_suffix('}')?))
}
fn expand(text: &str, aliases: &Aliases, stack: &mut Vec<String>) -> Result<Vec<String>, String> {
    if stack.len() >= 16 {
        return Err("bus alias/group nesting limit exceeded".into());
    }
    let mut out = vec![];
    if let Some(members) = aliases.get(text) {
        if members.is_empty() || stack.iter().any(|n| n == text) {
            return Err(format!(
                "Bus alias '{text}' is empty, conflicting or recursive"
            ));
        }
        stack.push(text.to_string());
        for member in members {
            out.extend(expand(member, aliases, stack)?);
            if out.len() > MAX_MEMBERS {
                return Err("bus member limit exceeded".into());
            }
        }
        stack.pop();
    } else if let Some((prefix, body)) = group(text) {
        stack.push(text.to_string());
        for member in split_members(body)? {
            for name in expand(member, aliases, stack)? {
                out.push(if prefix.is_empty() {
                    name
                } else {
                    format!("{prefix}.{name}")
                });
            }
            if out.len() > MAX_MEMBERS {
                return Err("bus member limit exceeded".into());
            }
        }
        stack.pop();
    } else if let Some((prefix, a, b)) = vector(text) {
        let (lo, hi) = (a.min(b), a.max(b));
        if hi - lo >= MAX_MEMBERS {
            return Err("bus member limit exceeded".into());
        }
        out.extend((lo..=hi).map(|i| format!("{prefix}{i}")));
    } else {
        out.push(text.to_string());
    }
    if out.is_empty() {
        return Err("bus has no members".into());
    }
    Ok(out)
}

/// (relative member key, scalar net name). Different group prefixes map by member name.
fn members(text: &str, aliases: &Aliases) -> Result<Option<Vec<(String, String)>>, String> {
    let is_bus = group(text).is_some() || vector(text).is_some();
    if is_bus && text.contains(['"', '\\']) {
        return Err("quoted or escaped bus names are not supported yet".into());
    }
    if let Some((prefix, body)) = group(text) {
        let mut out = BTreeMap::new();
        for token in split_members(body)? {
            for member in expand(token, aliases, &mut vec![])? {
                out.insert(
                    member.clone(),
                    if prefix.is_empty() {
                        member
                    } else {
                        format!("{prefix}.{member}")
                    },
                );
                if out.len() > MAX_MEMBERS {
                    return Err("bus member limit exceeded".into());
                }
            }
        }
        if out.is_empty() {
            return Err("bus has no members".into());
        }
        return Ok(Some(out.into_iter().collect()));
    }
    if let Some((_, a, b)) = vector(text) {
        let names = expand(text, aliases, &mut vec![])?;
        return Ok(Some(
            (a.min(b)..=a.max(b))
                .zip(names)
                .map(|(i, name)| (i.to_string(), name))
                .collect(),
        ));
    }
    Ok(None)
}

pub(super) struct Signal {
    pub element: usize,
    pub group: usize,
    pub key: String,
    pub name: String,
    pub scope: Option<LabelScope>,
}
pub(super) struct Buses {
    pub signals: Vec<Signal>,
    pub ports: BTreeSet<usize>,
}

pub(super) fn build(
    doc: &Document,
    conn: &SheetConn,
    aliases: &Aliases,
    file: &str,
    report: &mut Report,
) -> Buses {
    let segments: Vec<_> = doc
        .items()
        .filter_map(|(_, i)| {
            if let Item::Bus(b) = i {
                Some((b.a, b.b))
            } else {
                None
            }
        })
        .collect();
    let junctions: Vec<_> = doc
        .items()
        .filter_map(|(_, i)| {
            if let Item::Junction(j) = i {
                Some(j.pos)
            } else {
                None
            }
        })
        .collect();
    let mut uf = UnionFind::new(segments.len());
    for (i, &(a, b)) in segments.iter().enumerate() {
        for (j, &(c, d)) in segments.iter().enumerate().skip(i + 1) {
            if on_segment(a, c, d)
                || on_segment(b, c, d)
                || on_segment(c, a, b)
                || on_segment(d, a, b)
                || junctions
                    .iter()
                    .any(|&p| on_segment(p, a, b) && on_segment(p, c, d))
            {
                uf.union(i, j);
            }
        }
    }
    let roots: Vec<_> = (0..segments.len()).map(|i| uf.find(i)).collect();
    let mut out = Buses {
        signals: vec![],
        ports: BTreeSet::new(),
    };
    for (element, el) in conn.elems.iter().enumerate() {
        let (name, scope) = match &el.kind {
            ElemKind::Label { scope, name } => (*name, Some(*scope)),
            ElemKind::SheetEntry => {
                let Some(name) = conn.sheets.iter().find_map(|s| {
                    s.entries
                        .iter()
                        .position(|&e| e == element)
                        .map(|i| s.sheet.entries[i].name.as_str())
                }) else {
                    continue;
                };
                (name, None)
            }
            _ => continue,
        };
        let parsed = match members(name.trim(), aliases) {
            Ok(Some(m)) => m,
            Ok(None) => continue,
            Err(error) => {
                report.error("bus-members", error, file, vec![el.item], Some(el.a));
                out.ports.insert(element);
                continue;
            }
        };
        out.ports.insert(element);
        let groups: BTreeSet<_> = segments
            .iter()
            .enumerate()
            .filter(|(_, (a, b))| on_segment(el.a, *a, *b))
            .map(|(i, _)| roots[i])
            .collect();
        if groups.len() > 1 {
            report.error(
                "bus-connection",
                "Bus port lies at an unjoined crossing; add a junction or move the port".into(),
                file,
                vec![el.item],
                Some(el.a),
            );
            continue;
        }
        let group = groups.first().copied().unwrap_or(segments.len() + element);
        for (key, name) in parsed {
            out.signals.push(Signal {
                element,
                group,
                key,
                name,
                scope,
            });
        }
    }
    // Disjoint port definitions cannot form a meaningful bus connection.
    let mut definitions = BTreeMap::<usize, BTreeMap<usize, BTreeSet<&str>>>::new();
    let mut names = BTreeMap::<usize, BTreeMap<usize, BTreeSet<&str>>>::new();
    for signal in &out.signals {
        definitions
            .entry(signal.group)
            .or_default()
            .entry(signal.element)
            .or_default()
            .insert(&signal.key);
        if signal.scope.is_some() {
            names
                .entry(signal.group)
                .or_default()
                .entry(signal.element)
                .or_default()
                .insert(&signal.name);
        }
    }
    let mut invalid = BTreeSet::new();
    for (&group, ports) in &definitions {
        if ports.len() > 1
            && ports.iter().any(|(element, members)| {
                !ports.iter().any(|(other, other_members)| {
                    element != other && !members.is_disjoint(other_members)
                })
            })
        {
            let element = *ports.keys().next().unwrap();
            let el = &conn.elems[element];
            report.error(
                "bus-member-mismatch",
                "Connected bus ports have no common members".into(),
                file,
                vec![el.item],
                Some(el.a),
            );
            invalid.insert(group);
        }
    }
    for (&group, ports) in &names {
        if let Some(first) = ports.values().next() {
            if ports.values().any(|members| members != first) {
                let el = &conn.elems[*ports.keys().next().unwrap()];
                report.error(
                    "bus-name-conflict",
                    "A bus has conflicting label names; use one member definition".into(),
                    file,
                    vec![el.item],
                    Some(el.a),
                );
                invalid.insert(group);
            }
        }
    }
    out.signals
        .retain(|signal| !invalid.contains(&signal.group));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn groups_vectors_aliases_and_limits() {
        let aliases = Aliases::from([("LINK".into(), vec!["P".into(), "N".into()])]);
        assert_eq!(
            members("HOST{LINK}", &aliases).unwrap().unwrap(),
            vec![("N".into(), "HOST.N".into()), ("P".into(), "HOST.P".into())]
        );
        assert_eq!(
            members("DATA[2..0]", &aliases)
                .unwrap()
                .unwrap()
                .iter()
                .map(|(_, n)| n.as_str())
                .collect::<Vec<_>>(),
            vec!["DATA0", "DATA1", "DATA2"]
        );
        assert!(members("D[0..999999999999]", &aliases).is_err());
        assert!(members("~{RESET}", &aliases).unwrap().is_none());
        assert!(members("BUS{\"space name\"}", &aliases).is_err());
        let cycles = Aliases::from([("LOOP".into(), vec!["LOOP".into()])]);
        assert!(members("X{LOOP}", &cycles).is_err());
    }
}
