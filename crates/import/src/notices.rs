//! Preserve notice files beside a user-selected library without guessing redistribution rights.
use std::{fs,path::Path};
fn recognized(name: &str) -> bool {
    let upper=name.to_ascii_uppercase();
    ["LICENSE","LICENCE","NOTICE","COPYING","COPYRIGHT"].iter().any(|base|upper==*base||upper.starts_with(&format!("{base}."))||upper.starts_with(&format!("{base}-"))||upper.starts_with(&format!("{base}_")))
}
pub fn collect(source: &Path) -> Vec<String> {
    let directory=if source.is_dir(){source}else{source.parent().unwrap_or(source)};
    let Ok(entries)=fs::read_dir(directory) else{return vec![]};
    let mut files=entries.flatten().map(|e|e.path()).filter(|p|p.file_name().is_some_and(|n|recognized(&n.to_string_lossy()))).collect::<Vec<_>>();files.sort();
    files.into_iter().take(32).filter_map(|path|{
        let meta=fs::symlink_metadata(&path).ok()?;if !meta.is_file()||meta.file_type().is_symlink()||meta.len()>256_000{return None}
        let text=fs::read_to_string(&path).ok()?;if text.trim().is_empty(){return None}
        Some(format!("Source notice file: {}\n{}",path.file_name()?.to_string_lossy(),text))
    }).collect()
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn preserves_adjacent_notices_without_importing_unrelated_files(){
  let dir=std::env::temp_dir().join(format!("pcad-library-notice-{}-{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));fs::create_dir(&dir).unwrap();
  fs::write(dir.join("LICENSE.txt"),"Original author and full license").unwrap();fs::write(dir.join("private.txt"),"do not copy").unwrap();fs::write(dir.join("parts.arlib"),"{}").unwrap();
  let notices=collect(&dir.join("parts.arlib"));assert_eq!(notices.len(),1);assert!(notices[0].contains("Original author and full license"));assert!(!notices[0].contains(&dir.to_string_lossy().to_string()));
  if dir.starts_with(std::env::temp_dir())&&dir.file_name().unwrap().to_string_lossy().starts_with("pcad-library-notice-"){fs::remove_dir_all(dir).unwrap();}
 }
}
