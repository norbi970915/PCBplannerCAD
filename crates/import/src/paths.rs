//! Library data discovery by folder layout, with an explicit configuration override.
use std::path::{Path, PathBuf};

fn is_library_share(path: &Path) -> bool {
    ["symbols", "footprints", "3dmodels"].iter().any(|name| path.join(name).is_dir())
}

fn version_key(path: &Path) -> Vec<u32> {
    path.file_name().and_then(|n| n.to_str()).unwrap_or("").split('.').map(str::parse).collect::<Result<Vec<_>, _>>().unwrap_or_default()
}

/// Finds a library share beneath installation roots without relying on application names.
pub fn find_library_share(roots: &[PathBuf]) -> Option<PathBuf> {
    let directories = |path: &Path| std::fs::read_dir(path).into_iter().flatten().filter_map(Result::ok).map(|e| e.path()).filter(|p| p.is_dir()).collect::<Vec<_>>();
    let mut candidates: Vec<(Vec<u32>, PathBuf)> = Vec::new();
    for root in roots {
        let mut applications=directories(root);applications.push(root.clone());
        for app in applications {
            let mut releases=directories(&app).into_iter().filter(|p| !version_key(p).is_empty()).collect::<Vec<_>>();
            releases.push(app);
            for release in releases {
                let share=release.join("share");
                let mut folders=directories(&share);folders.push(share);
                for folder in folders {
                    if is_library_share(&folder) { candidates.push((version_key(&release),folder)); }
                }
            }
        }
    }
    candidates.sort();candidates.pop().map(|(_,path)|path)
}

/// PCAD_LIBRARY_ROOT selects a library data folder explicitly; otherwise discover installed data.
pub fn library_share() -> Option<PathBuf> {
    if let Some(path)=std::env::var_os("PCAD_LIBRARY_ROOT").map(PathBuf::from) {
        return is_library_share(&path).then_some(path);
    }
    #[cfg(windows)]
    {
        let roots=["ProgramFiles","ProgramFiles(x86)"].iter().filter_map(std::env::var_os).map(PathBuf::from).collect::<Vec<_>>();
        find_library_share(&roots)
    }
    #[cfg(not(windows))]
    { None }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn discovers_neutral_folder_layouts_and_chooses_the_newest_numeric_release() {
        let root=std::env::temp_dir().join(format!("pcad-library-discovery-{}",std::process::id()));
        let old=root.join("DesignData/9.9/share/library/symbols");
        let latest=root.join("DesignData/10.1/share/library/footprints");
        std::fs::create_dir_all(&old).unwrap();std::fs::create_dir_all(&latest).unwrap();
        std::fs::create_dir_all(root.join("OtherData/20.0/share/empty")).unwrap();
        assert_eq!(find_library_share(std::slice::from_ref(&root)),Some(latest.parent().unwrap().to_path_buf()));
        assert_eq!(find_library_share(&[root.join("missing")]),None);
        std::fs::remove_dir_all(root).unwrap();
    }
}
