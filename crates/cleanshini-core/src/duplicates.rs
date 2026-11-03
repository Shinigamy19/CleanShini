use crate::model::Result;
use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct DuplicateGroup {
    pub hash: String,
    pub size: u64,
    pub paths: Vec<PathBuf>,
}

/// Encuentra archivos duplicados por hash en las rutas dadas.
/// Primero agrupa por tamaño, luego hashea solo los candidatos (por chunks).
pub fn find_duplicates(roots: &[PathBuf]) -> Result<Vec<DuplicateGroup>> {
    // Group files by size
    let mut by_size: HashMap<u64, Vec<PathBuf>> = HashMap::new();
    for root in roots {
        if !root.exists() { continue; }
        for entry in walkdir::WalkDir::new(root).follow_links(false).into_iter().filter_map(|e| e.ok()) {
            let path = entry.path().to_path_buf();
            if path.is_dir() { continue; }
            let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
            if size == 0 { continue; }
            by_size.entry(size).or_default().push(path);
        }
    }

    let mut groups = Vec::new();
    for (size, paths) in by_size {
        if paths.len() < 2 { continue; }
        // Hash each file (chunked)
        let mut by_hash: HashMap<String, Vec<PathBuf>> = HashMap::new();
        for path in paths {
            if let Ok(mut f) = File::open(&path) {
                use sha2::{Sha256, Digest};
                let mut hasher = Sha256::new();
                let mut buf = vec![0u8; 64 * 1024];
                let mut ok = true;
                loop {
                    match f.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => hasher.update(&buf[..n]),
                        Err(_) => { ok = false; break; }
                    }
                }
                if ok {
                    let hash = format!("{:x}", hasher.finalize());
                    by_hash.entry(hash).or_default().push(path);
                }
            }
        }
        for (hash, paths) in by_hash {
            if paths.len() >= 2 {
                groups.push(DuplicateGroup { hash, size, paths });
            }
        }
    }
    // Sort by wasted space
    groups.sort_by(|a, b| (b.size * (b.paths.len() as u64 - 1)).cmp(&(a.size * (a.paths.len() as u64 - 1))));
    Ok(groups)
}
