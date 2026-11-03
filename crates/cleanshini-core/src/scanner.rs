use crate::model::{Category, RuleSet, ScanItem, Result};
use chrono::{DateTime, Utc};
use std::path::PathBuf;
use std::time::{Duration, SystemTime};
use walkdir::WalkDir;

pub fn scan(rules: &RuleSet) -> Result<Vec<ScanItem>> {
    let mut items: Vec<ScanItem> = Vec::new();
    let enabled: std::collections::HashSet<Category> = rules.categories.iter().copied().collect();
    let now = SystemTime::now();
    let min_age = Duration::from_secs(rules.min_age_days as u64 * 86400);

    for root in &rules.include_paths {
        if !root.exists() {
            continue;
        }
        // Walk files
        for entry in WalkDir::new(root).follow_links(false).into_iter().filter_map(|e| e.ok()) {
            let path = entry.path().to_path_buf();
            if path.is_dir() {
                continue;
            }
            // skip excluded paths
            if rules.exclude_paths.iter().any(|ex| path.starts_with(ex)) {
                continue;
            }
            let meta = match entry.metadata() {
                Ok(m) => m,
                Err(_) => continue,
            };
            let size = meta.len();
            if size < rules.min_size_bytes {
                continue;
            }
            if let Some(max) = rules.max_size_bytes {
                if size > max {
                    continue;
                }
            }
            let modified = meta.modified().unwrap_or(now);
            // extension filter
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                if rules.exclude_extensions.iter().any(|e| e.eq_ignore_ascii_case(ext)) {
                    continue;
                }
            }

            // classify
            let (cat, reason) = match classify(&path, root, modified, now, min_age, size, rules.large_min_bytes, &enabled) {
                Some(c) => c,
                None => continue,
            };
            if !enabled.contains(&cat) {
                continue;
            }
            // age filter for old files: only include if older than min_age when category is OldFile
            // (already handled in classify)
            items.push(ScanItem {
                path,
                size,
                modified,
                category: cat,
                reason,
            });
        }
    }
    // Additional pass: AI model files anywhere (by extension), pruned walk
    if enabled.contains(&Category::AIModel) {
        let model_exts = ["gguf", "safetensors", "ggml", "ckpt", "onnx", "tflite", "mlmodel", "bin", "pt", "pth", "pb"];
        let skip_dirs = ["windows", "program files", "program files (x86)", "programdata", "node_modules", ".git", "$recycle.bin", "system volume information", "winsxs", "appdata", "application data"];
        let mut roots = Vec::new();
        let mut depths: Vec<usize> = Vec::new();
        if let Some(home) = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")) {
            roots.push(PathBuf::from(home));
            depths.push(4);
        }
        // Selected drives
        for drive in &rules.selected_drives {
            let p = PathBuf::from(drive);
            if p.exists() {
                roots.push(p);
                depths.push(2);
            }
        }
        for (root, depth) in roots.iter().zip(depths.iter()) {
            for entry in WalkDir::new(root).max_depth(*depth).follow_links(false).into_iter().filter_entry(|e| {
                if e.file_type().is_dir() {
                    let n = e.file_name().to_string_lossy().to_lowercase();
                    !skip_dirs.iter().any(|s| n == *s)
                } else {
                    true
                }
            }).filter_map(|e| e.ok()) {
                let path = entry.path().to_path_buf();
                if path.is_dir() { continue; }
                let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
                if !model_exts.contains(&ext.as_str()) { continue; }
                // for generic exts, require size threshold
                if ext == "bin" || ext == "pt" || ext == "pth" || ext == "pb" {
                    let sz = entry.metadata().map(|m| m.len()).unwrap_or(0);
                    if sz < 50 * 1024 * 1024 { continue; }
                }
                if rules.exclude_paths.iter().any(|ex| path.starts_with(ex)) { continue; }
                let meta = match entry.metadata() { Ok(m) => m, Err(_) => continue };
                let size = meta.len();
                if let Some(max) = rules.max_size_bytes { if size > max { continue; } }
                let modified = meta.modified().unwrap_or(now);
                items.push(ScanItem {
                    path,
                    size,
                    modified,
                    category: Category::AIModel,
                    reason: format!("modelo de IA (.{})", ext),
                });
            }
        }
    }

    // Additional pass: large files in user folders
    if enabled.contains(&Category::Large) && rules.large_min_bytes > 0 {
        for root in &rules.large_scan_paths {
            if !root.exists() { continue; }
            for entry in WalkDir::new(root).follow_links(false).into_iter().filter_map(|e| e.ok()) {
                let path = entry.path().to_path_buf();
                if path.is_dir() { continue; }
                if rules.exclude_paths.iter().any(|ex| path.starts_with(ex)) { continue; }
                let meta = match entry.metadata() { Ok(m) => m, Err(_) => continue };
                let size = meta.len();
                if size < rules.large_min_bytes { continue; }
                if let Some(max) = rules.max_size_bytes { if size > max { continue; } }
                if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                    if rules.exclude_extensions.iter().any(|e| e.eq_ignore_ascii_case(ext)) { continue; }
                }
                let modified = meta.modified().unwrap_or(now);
                // Classify large files: if it's a known model extension, mark as AIModel
                let cat = {
                    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
                    let model_exts = ["gguf", "safetensors", "ggml", "ckpt", "onnx", "tflite", "mlmodel", "bin", "pt", "pth", "pb"];
                    let is_generic = ext == "bin" || ext == "pt" || ext == "pth" || ext == "pb";
                    let is_model = model_exts.contains(&ext.as_str()) && (!is_generic || size >= 50 * 1024 * 1024);
                    if is_model { Category::AIModel } else { Category::Large }
                };
                let reason = if cat == Category::AIModel {
                    format!("modelo de IA (>= {})", format_size(rules.large_min_bytes))
                } else {
                    format!("archivo grande (>= {})", format_size(rules.large_min_bytes))
                };
                items.push(ScanItem {
                    path,
                    size,
                    modified,
                    category: cat,
                    reason,
                });
            }
        }
    }

    // dedup by path
    items.sort_by(|a, b| a.path.cmp(&b.path));
    items.dedup_by(|a, b| a.path == b.path);
    Ok(items)
}

fn classify(path: &PathBuf, _root: &PathBuf, modified: SystemTime, now: SystemTime, min_age: Duration, size: u64, large_min: u64, _enabled: &std::collections::HashSet<Category>) -> Option<(Category, String)> {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_lowercase();
    let path_str = path.to_string_lossy().to_lowercase();
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();

    // Windows Update cache
    if path_str.contains("softwaredistribution") && path_str.contains("download") {
        return Some((Category::WindowsUpdate, "caché de Windows Update".into()));
    }
    // Windows.old
    if path_str.contains("windows.old") {
        return Some((Category::WindowsOld, "instalación anterior de Windows".into()));
    }
    // Crash dumps / WER
    if path_str.contains("minidump") || path_str.contains("livekernelreports") || path_str.contains("crashdumps") || path_str.contains("\\wer\\") {
        return Some((Category::CrashDump, "volcado de memoria / reporte de errores".into()));
    }
    // App caches
    if path_str.contains("spotify\\storage") || path_str.contains("discord\\cache") || path_str.contains("microsoft\\teams\\cache") || path_str.contains("slack\\cache") {
        return Some((Category::AppCache, "caché de aplicación".into()));
    }
    // Dev caches
    if path_str.contains(".npm") || path_str.contains("_cacache") || path_str.contains(".cargo\\registry") || path_str.contains(".gradle\\caches") || path_str.contains(".nuget\\packages") || path_str.contains(".yarn\\cache") || path_str.contains("pip\\cache") || path_str.contains(".pnpm-store") {
        return Some((Category::DevCache, "caché de desarrollo".into()));
    }
    // AI model caches / files
    if path_str.contains(".ollama") || path_str.contains("huggingface") || path_str.contains("lm-studio") || path_str.contains("gpt4all") || path_str.contains("\\.jan\\") {
        return Some((Category::AIModel, "modelo de IA local".into()));
    }
    // AI framework install dirs
    if path_str.contains("comfyui") || path_str.contains("stable-diffusion") || path_str.contains("sd-webui")
        || path_str.contains("text-generation-webui") || path_str.contains("kobold") || path_str.contains("oobabooga")
        || path_str.contains("invokeai") || path_str.contains("automatic1111") || path_str.contains("llama") {
        // classify model files inside
        let exts = ["ckpt","safetensors","pt","pth","bin","gguf","ggml","onnx"];
        if exts.contains(&ext.as_str()) {
            return Some((Category::AIModel, format!("modelo de IA (.{})", ext)));
        }
    }
    // Virtual disks
    if ["vhd","vhdx","vmdk","qcow2","vdi"].contains(&ext.as_str()) {
        return Some((Category::VirtualDisk, format!("disco virtual ({})", ext)));
    }
    if path_str.contains("docker\\wsl") || path_str.contains("ext4.vhdx") {
        return Some((Category::VirtualDisk, "disco virtual de Docker/WSL".into()));
    }
    let model_exts = ["gguf", "safetensors", "ggml", "ckpt", "onnx", "tflite", "mlmodel"];
    if model_exts.contains(&ext.as_str()) {
        return Some((Category::AIModel, format!("modelo de IA (.{})", ext)));
    }
    // large .bin/.pt/.pth only if big
    if (ext == "bin" || ext == "pt" || ext == "pth" || ext == "pb") && size >= 50 * 1024 * 1024 {
        return Some((Category::AIModel, format!("modelo de IA (.{})", ext)));
    }
    // Temp
    if path_str.contains("\\temp\\") || path_str.contains("/temp/") || path_str.contains("\\tmp\\") {
        return Some((Category::Temp, "ubicado en carpeta Temp".into()));
    }
    if name.starts_with("~") || ext == "tmp" || ext == "temp" || ext == "crdownload" {
        return Some((Category::Temp, format!("archivo temporal (.{})", ext)));
    }

    // Cache
    if path_str.contains("cache") {
        return Some((Category::Cache, "ubicado en carpeta de caché".into()));
    }
    if name == "thumbs.db" || name == "desktop.ini" {
        return Some((Category::Cache, "archivo de caché del sistema".into()));
    }
    // browser caches
    if path_str.contains("chrome") && path_str.contains("cache") { return Some((Category::Cache, "caché de Chrome".into())); }
    if path_str.contains("edge") && path_str.contains("cache") { return Some((Category::Cache, "caché de Edge".into())); }
    if path_str.contains("firefox") && path_str.contains("cache") { return Some((Category::Cache, "caché de Firefox".into())); }

    // Log
    if ext == "log" || ext == "old" || ext == "bak" || ext == "dmp" {
        return Some((Category::Log, format!("archivo de log/respaldo (.{})", ext)));
    }
    if path_str.contains("\\logs\\") || path_str.contains("/logs/") {
        return Some((Category::Log, "ubicado en carpeta de logs".into()));
    }

    // Residue: empty-ish leftover patterns — directories with uninstaller leftovers
    if path_str.contains("uninstall") {
        return Some((Category::Residue, "residuo de desinstalación".into()));
    }

    // OldFile: apply age threshold
    if min_age.as_secs() > 0 {
        if let Ok(age) = now.duration_since(modified) {
            if age >= min_age {
                return Some((Category::OldFile, format!("sin modificar hace {} días", age.as_secs() / 86400)));
            }
        }
    }

    // Large files
    if large_min > 0 && size >= large_min {
        return Some((Category::Large, format!("archivo grande (>= {})", format_size(large_min))));
    }

    None
}

pub fn format_size(bytes: u64) -> String {
    if bytes >= 1024 * 1024 * 1024 {
        format!("{:.2} GB", bytes as f64 / (1024.0*1024.0*1024.0))
    } else if bytes >= 1024 * 1024 {
        format!("{:.2} MB", bytes as f64 / (1024.0*1024.0))
    } else if bytes >= 1024 {
        format!("{:.2} KB", bytes as f64 / 1024.0)
    } else {
        format!("{} B", bytes)
    }
}

// helper to keep DateTime import used
#[allow(dead_code)]
fn _touch(_d: DateTime<Utc>) {}
