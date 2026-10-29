//! Saved-checkpoint differences. Comparison never edits the working design.
use super::*;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RevisionChanges {
    pub from: RevisionInfo,
    pub to: RevisionInfo,
    pub added: usize,
    pub removed: usize,
    pub modified: usize,
    pub files: Vec<FileChange>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileChange {
    pub path: String,
    pub status: String,
    pub summary: String,
    pub details: Vec<String>,
    pub omitted: usize,
}
struct Index {
    root: PathBuf,
    manifest: Manifest,
}
fn index(project: &Path, id: &str) -> Res<Index> {
    folder_name(id)?;
    let history = history_dir(&project_root(project)?)?;
    let root = checked_dir(&history, Path::new(id))?;
    let manifest: Manifest =
        serde_json::from_slice(&fs::read(root.join("manifest.json")).map_err(err)?).map_err(err)?;
    if manifest.version != 1
        || manifest.info.id != id
        || manifest.info.files != manifest.files.len()
        || manifest.files.len() > 16384
    {
        return Err("Invalid revision manifest".into());
    }
    storage::relative_path(&manifest.project).map_err(err)?;
    let mut paths = BTreeSet::new();
    let mut total = 0u64;
    for f in &manifest.files {
        storage::relative_path(&f.path).map_err(err)?;
        if !paths.insert(f.path.to_string_lossy().to_lowercase()) {
            return Err("Duplicate checkpoint file".into());
        }
        total = total
            .checked_add(f.bytes)
            .ok_or("Invalid checkpoint size")?;
        if total > 2 * 1024 * 1024 * 1024 {
            return Err("Checkpoint exceeds supported size".into());
        }
    }
    if total != manifest.info.bytes {
        return Err("Checkpoint byte count is inconsistent".into());
    }
    let out = Index { root, manifest };
    for f in &out.manifest.files {
        out.verify(f)?;
    }
    Ok(out)
}
impl Index {
    fn verify(&self, f: &SavedFile) -> Res<()> {
        let path = fs::canonicalize(self.root.join("files").join(&f.path)).map_err(err)?;
        if !path.starts_with(&self.root) {
            return Err("Checkpoint file escapes its folder".into());
        }
        let mut reader = fs::File::open(path).map_err(err)?.take(f.bytes + 1);
        let mut buffer = [0u8; 65536];
        let mut size = 0u64;
        let mut hash = 0xcbf29ce484222325u64;
        loop {
            let count = reader.read(&mut buffer).map_err(err)?;
            if count == 0 {
                break;
            }
            size += count as u64;
            for byte in &buffer[..count] {
                hash = (hash ^ (*byte as u64)).wrapping_mul(0x100000001b3);
            }
        }
        if size != f.bytes || format!("{hash:016x}") != f.checksum {
            return Err(format!(
                "Checkpoint file {} is damaged; comparison cancelled",
                f.path.display()
            ));
        }
        Ok(())
    }
    fn read(&self, f: &SavedFile) -> Res<Vec<u8>> {
        let path = fs::canonicalize(self.root.join("files").join(&f.path)).map_err(err)?;
        if !path.starts_with(&self.root) {
            return Err("Checkpoint file escapes its folder".into());
        }
        let mut bytes = Vec::new();
        fs::File::open(path)
            .map_err(err)?
            .take(f.bytes + 1)
            .read_to_end(&mut bytes)
            .map_err(err)?;
        if bytes.len() as u64 != f.bytes || checksum(&bytes) != f.checksum {
            return Err(format!(
                "Checkpoint file {} is damaged; comparison cancelled",
                f.path.display()
            ));
        }
        Ok(bytes)
    }
}
struct Lines {
    values: Vec<String>,
    omitted: usize,
}
impl Lines {
    fn push(&mut self, line: String) {
        if self.values.len() < 200 {
            self.values.push(line);
        } else {
            self.omitted += 1;
        }
    }
}
fn short(text: String) -> String {
    let mut chars = text.chars();
    let head: String = chars.by_ref().take(160).collect();
    if chars.next().is_some() {
        head + "…"
    } else {
        head
    }
}
fn brief(v: &Value, key: &str) -> String {
    if let Some(obj) = v.as_object() {
        if let (Some(x), Some(y)) = (
            obj.get("x").and_then(Value::as_f64),
            obj.get("y").and_then(Value::as_f64),
        ) {
            return format!("({:.4}, {:.4}) mm", x / 1e6, y / 1e6);
        }
    }
    if let Some(n) = v.as_f64() {
        if matches!(
            key,
            "width"
                | "height"
                | "drill"
                | "diameter"
                | "radius"
                | "clearance"
                | "minWidth"
                | "thermalGap"
                | "spokeWidth"
                | "targetLength"
        ) {
            return format!("{:.4} mm", n / 1e6);
        }
        if matches!(key, "rotation" | "startAngle" | "sweep") {
            return format!("{n}°");
        }
    }
    if let Some(a) = v.as_array() {
        if a.len() > 8 {
            return format!("{} entries (content changed)", a.len());
        }
    }
    short(v.to_string())
}
fn named<'a>(array: &'a [Value]) -> Option<BTreeMap<String, &'a Value>> {
    let mut result = BTreeMap::new();
    for v in array {
        let key = v.get("name")?.as_str()?.to_string();
        if result.insert(key, v).is_some() {
            return None;
        }
    }
    Some(result)
}
fn label(path: &str) -> String {
    path.replace("fields.", "")
        .replace("parameters.", "parameter ")
        .replace(".value", "")
        .replace("padNets.", "pad net ")
}
fn props(old: &Value, new: &Value, prefix: &str, depth: usize, out: &mut Lines) {
    if old == new {
        return;
    }
    if depth < 5 {
        if let (Some(a), Some(b)) = (old.as_object(), new.as_object()) {
            if !(a.contains_key("x")
                && a.contains_key("y")
                && b.contains_key("x")
                && b.contains_key("y"))
            {
                let keys: BTreeSet<_> = a.keys().chain(b.keys()).collect();
                for key in keys {
                    let name = if prefix.is_empty() {
                        key.clone()
                    } else {
                        format!("{prefix}.{key}")
                    };
                    match (a.get(key), b.get(key)) {
                        (Some(x), Some(y)) => props(x, y, &name, depth + 1, out),
                        (None, Some(y)) => {
                            out.push(format!("{} added: {}", label(&name), brief(y, key)))
                        }
                        (Some(x), None) => {
                            out.push(format!("{} removed: {}", label(&name), brief(x, key)))
                        }
                        _ => {}
                    }
                }
                return;
            }
        }
        if let (Some(a), Some(b)) = (old.as_array(), new.as_array()) {
            if let (Some(aa), Some(bb)) = (named(a), named(b)) {
                let keys: BTreeSet<_> = aa.keys().chain(bb.keys()).collect();
                for key in keys {
                    let name = format!("{prefix}.{key}");
                    match (aa.get(key), bb.get(key)) {
                        (Some(x), Some(y)) => props(x, y, &name, depth + 1, out),
                        (None, Some(_)) => out.push(format!("{} added", label(&name))),
                        (Some(_), None) => out.push(format!("{} removed", label(&name))),
                        _ => {}
                    }
                }
                let old_order: Vec<_> = a
                    .iter()
                    .filter_map(|v| v.get("name").and_then(Value::as_str))
                    .collect();
                let new_order: Vec<_> = b
                    .iter()
                    .filter_map(|v| v.get("name").and_then(Value::as_str))
                    .collect();
                if aa.keys().eq(bb.keys()) && old_order != new_order {
                    out.push(format!("{} order changed", label(prefix)));
                }
                return;
            }
        }
    }
    let key = prefix.rsplit('.').next().unwrap_or(prefix);
    let (a, b) = (brief(old, key), brief(new, key));
    out.push(if a == b {
        format!("{} content changed", label(prefix))
    } else {
        format!("{}: {a} → {b}", label(prefix))
    });
}
fn entity(value: &Value, key: &str) -> String {
    let kind = value
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or("object");
    let designation = value
        .get("designator")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            value
                .get("fields")
                .and_then(Value::as_array)
                .and_then(|a| {
                    a.iter()
                        .find(|f| f.get("name").and_then(Value::as_str) == Some("Designator"))
                })
                .and_then(|f| f.get("value"))
                .and_then(Value::as_str)
                .map(str::to_string)
        });
    if let Some(d) = designation {
        format!("{kind} {d} (#{key})")
    } else {
        let net = value
            .get("net")
            .and_then(Value::as_str)
            .filter(|n| !n.is_empty());
        format!(
            "{kind} #{key}{}",
            net.map(|n| format!(" on {n}")).unwrap_or_default()
        )
    }
}
fn file_change(path: &Path, a: &[u8], b: &[u8]) -> FileChange {
    let mut lines = Lines {
        values: vec![],
        omitted: 0,
    };
    let mut summary = "File contents changed".to_string();
    let ext = path
        .extension()
        .and_then(|x| x.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if matches!(
        ext.as_str(),
        "arsch" | "arpcb" | "arlib" | "arproj" | "json"
    ) {
        if let (Ok(mut old), Ok(mut new)) = (
            serde_json::from_slice::<Value>(a),
            serde_json::from_slice::<Value>(b),
        ) {
            if old == new {
                summary = "Formatting changed; saved data is unchanged".into();
            } else if matches!(ext.as_str(), "arsch" | "arpcb") {
                let parse = |v: &Value| -> Option<BTreeMap<String, Value>> {
                    let mut map = BTreeMap::new();
                    for item in v.get("items")?.as_array()? {
                        let id = item.get("id")?.as_u64()?.to_string();
                        if map.insert(id, item.get("item")?.clone()).is_some() {
                            return None;
                        }
                    }
                    Some(map)
                };
                if let (Some(aa), Some(bb)) = (parse(&old), parse(&new)) {
                    let mut counts = [0, 0, 0];
                    let keys: BTreeSet<_> = aa.keys().chain(bb.keys()).collect();
                    for key in keys {
                        match (aa.get(key), bb.get(key)) {
                            (None, Some(y)) => {
                                counts[0] += 1;
                                lines.push(format!("Added {}", entity(y, key)));
                            }
                            (Some(x), None) => {
                                counts[1] += 1;
                                lines.push(format!("Removed {}", entity(x, key)));
                            }
                            (Some(x), Some(y)) if x != y => {
                                counts[2] += 1;
                                let mut details = Lines {
                                    values: vec![],
                                    omitted: 0,
                                };
                                props(x, y, "", 0, &mut details);
                                let name = entity(y, key);
                                for detail in details.values {
                                    lines.push(format!("{name}: {detail}"));
                                }
                                lines.omitted += details.omitted;
                            }
                            _ => {}
                        }
                    }
                    summary = format!(
                        "{} objects added, {} removed, {} modified",
                        counts[0], counts[1], counts[2]
                    );
                    old.as_object_mut().unwrap().remove("items");
                    new.as_object_mut().unwrap().remove("items");
                    props(&old, &new, "document", 0, &mut lines);
                    if counts == [0, 0, 0] && !lines.values.is_empty() {
                        summary = "Document definitions or metadata changed".into();
                    }
                } else {
                    props(&old, &new, "document", 0, &mut lines);
                }
            } else {
                props(&old, &new, "", 0, &mut lines);
                summary = if ext == "arlib" {
                    "Library definitions changed"
                } else if ext == "arproj" {
                    "Project settings or linked files changed"
                } else {
                    "Saved data changed"
                }
                .into();
            }
        } else {
            lines.push("File content changed; structured details are unavailable".into());
        }
    } else {
        lines.push(format!(
            "Asset contents changed: {} → {} bytes",
            a.len(),
            b.len()
        ));
    }
    FileChange {
        path: path.to_string_lossy().replace('\\', "/"),
        status: "modified".into(),
        summary,
        details: lines.values,
        omitted: lines.omitted,
    }
}
pub fn compare_revisions(project: &Path, from: &str, to: &str) -> Res<RevisionChanges> {
    let a = index(project, from)?;
    let b = index(project, to)?;
    let aa: BTreeMap<_, _> = a.manifest.files.iter().map(|f| (&f.path, f)).collect();
    let bb: BTreeMap<_, _> = b.manifest.files.iter().map(|f| (&f.path, f)).collect();
    let keys: BTreeSet<_> = aa.keys().chain(bb.keys()).copied().collect();
    let mut files = vec![];
    let (mut added, mut removed, mut modified) = (0, 0, 0);
    for key in keys {
        match (aa.get(key), bb.get(key)) {
            (None, Some(_)) => {
                added += 1;
                files.push(FileChange {
                    path: key.to_string_lossy().replace('\\', "/"),
                    status: "added".into(),
                    summary: "File added".into(),
                    details: vec![],
                    omitted: 0,
                });
            }
            (Some(_), None) => {
                removed += 1;
                files.push(FileChange {
                    path: key.to_string_lossy().replace('\\', "/"),
                    status: "removed".into(),
                    summary: "File removed".into(),
                    details: vec![],
                    omitted: 0,
                });
            }
            (Some(x), Some(y)) if x.checksum != y.checksum || x.bytes != y.bytes => {
                modified += 1;
                let structured = key.extension().and_then(|s| s.to_str()).is_some_and(|s| {
                    matches!(
                        s.to_ascii_lowercase().as_str(),
                        "arsch" | "arpcb" | "arlib" | "arproj" | "json"
                    )
                });
                if structured && x.bytes <= 64 * 1024 * 1024 && y.bytes <= 64 * 1024 * 1024 {
                    files.push(file_change(key, &a.read(x)?, &b.read(y)?));
                } else {
                    files.push(FileChange {
                        path: key.to_string_lossy().replace('\\', "/"),
                        status: "modified".into(),
                        summary: if structured {
                            "Large saved document changed; detailed view omitted"
                        } else {
                            "Asset contents changed"
                        }
                        .into(),
                        details: vec![format!("Saved size: {} → {} bytes", x.bytes, y.bytes)],
                        omitted: 0,
                    });
                }
            }
            _ => {}
        }
    }
    Ok(RevisionChanges {
        from: a.manifest.info,
        to: b.manifest.info,
        added,
        removed,
        modified,
        files,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn readable_component_track_rule_and_library_differences() {
        let a = serde_json::json!({"items":[{"id":1,"item":{"kind":"footprint","designator":"R1","pos":{"x":0,"y":0},"value":"10k","padNets":{"1":"OLD"}}},{"id":2,"item":{"kind":"track","net":"SIG","width":200000,"a":{"x":0,"y":0},"b":{"x":1000000,"y":0}}}]});
        let b = serde_json::json!({"items":[{"id":1,"item":{"kind":"footprint","designator":"R1","pos":{"x":2000000,"y":0},"value":"22k","padNets":{"1":"NEW"}}},{"id":3,"item":{"kind":"via","net":"SIG"}}]});
        let c = file_change(
            Path::new("Board.arpcb"),
            &serde_json::to_vec(&a).unwrap(),
            &serde_json::to_vec(&b).unwrap(),
        );
        let details = c.details.join("\n");
        assert!(c.summary.contains("1 objects added, 1 removed, 1 modified"));
        assert!(details.contains("R1"));
        assert!(details.contains("2.0000"));
        assert!(details.contains("10k") && details.contains("22k"));
        assert!(details.contains("pad net 1"));
        assert!(details.contains("Removed track #2") && details.contains("Added via #3"));
        let a = serde_json::json!({"footprints":[{"name":"PKG","model":{"path":"old.step","offset":[0,0,0]}}]});
        let b = serde_json::json!({"footprints":[{"name":"PKG","model":{"path":"new.step","offset":[0,0,1]}}]});
        let c = file_change(
            Path::new("Parts.arlib"),
            &serde_json::to_vec(&a).unwrap(),
            &serde_json::to_vec(&b).unwrap(),
        );
        assert!(
            c.details
                .iter()
                .any(|d| d.contains("model.path") && d.contains("new.step"))
        );
    }
    #[test]
    fn formatting_is_distinguished_and_reordered_rules_are_reported() {
        let a = br#"{"name":"Board","rules":[{"name":"A"},{"name":"B"}]}"#;
        let v: Value = serde_json::from_slice(a).unwrap();
        let pretty = serde_json::to_vec_pretty(&v).unwrap();
        assert!(
            file_change(Path::new("Test.arproj"), a, &pretty)
                .summary
                .contains("Formatting")
        );
        let b = br#"{"name":"Board","rules":[{"name":"B"},{"name":"A"}]}"#;
        assert!(
            file_change(Path::new("Test.json"), a, b)
                .details
                .iter()
                .any(|s| s.contains("order changed"))
        );
    }
}
