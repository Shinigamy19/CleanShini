use crate::model::{RuleSet, Result};
use directories::ProjectDirs;
use std::path::PathBuf;

pub fn config_dir() -> PathBuf {
    if let Some(proj) = ProjectDirs::from("com", "cleanshini", "CleanShini") {
        proj.config_dir().to_path_buf()
    } else {
        PathBuf::from(".cleanshini")
    }
}

pub fn data_dir() -> PathBuf {
    if let Some(proj) = ProjectDirs::from("com", "cleanshini", "CleanShini") {
        proj.data_dir().to_path_buf()
    } else {
        PathBuf::from(".cleanshini")
    }
}

pub fn rules_path() -> PathBuf {
    config_dir().join("rules.json")
}

pub fn last_plan_path() -> PathBuf {
    data_dir().join("last-plan.json")
}

pub fn audit_path() -> PathBuf {
    data_dir().join("audit.jsonl")
}

pub fn load_rules() -> RuleSet {
    let p = rules_path();
    if p.exists() {
        if let Ok(s) = std::fs::read_to_string(&p) {
            if let Ok(mut r) = serde_json::from_str::<RuleSet>(&s) {
                // Merge with defaults: add new default categories/paths that are missing
                let def = RuleSet::default();
                for c in &def.categories {
                    if !r.categories.contains(c) {
                        r.categories.push(*c);
                    }
                }
                for path in &def.include_paths {
                    if !r.include_paths.contains(path) {
                        r.include_paths.push(path.clone());
                    }
                }
                for path in &def.large_scan_paths {
                    if !r.large_scan_paths.contains(path) {
                        r.large_scan_paths.push(path.clone());
                    }
                }
                if r.selected_drives.is_empty() {
                    r.selected_drives = def.selected_drives.clone();
                }
                return r;
            }
        }
    }
    RuleSet::default()
}

pub fn save_rules(rules: &RuleSet) -> Result<()> {
    let dir = config_dir();
    std::fs::create_dir_all(&dir)?;
    let s = serde_json::to_string_pretty(rules)?;
    std::fs::write(rules_path(), s)?;
    Ok(())
}

pub fn ensure_dirs() -> Result<()> {
    std::fs::create_dir_all(config_dir())?;
    std::fs::create_dir_all(data_dir())?;
    Ok(())
}
