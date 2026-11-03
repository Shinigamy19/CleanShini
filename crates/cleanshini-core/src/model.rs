use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::SystemTime;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Category {
    Temp,
    Cache,
    Residue,
    Log,
    OldFile,
    Large,
    RecycleBin,
    WindowsUpdate,
    WindowsOld,
    CrashDump,
    AppCache,
    DevCache,
    AIModel,
    VirtualDisk,
    Custom,
}

impl Category {
    pub fn as_str(&self) -> &'static str {
        match self {
            Category::Temp => "temp",
            Category::Cache => "cache",
            Category::Residue => "residue",
            Category::Log => "log",
            Category::OldFile => "oldfile",
            Category::Large => "large",
            Category::RecycleBin => "recyclebin",
            Category::WindowsUpdate => "winupdate",
            Category::WindowsOld => "windowsold",
            Category::CrashDump => "crashdump",
            Category::AppCache => "appcache",
            Category::DevCache => "devcache",
            Category::AIModel => "aimodel",
            Category::VirtualDisk => "virtualdisk",
            Category::Custom => "custom",
        }
    }
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "temp" => Some(Category::Temp),
            "cache" => Some(Category::Cache),
            "residue" => Some(Category::Residue),
            "log" => Some(Category::Log),
            "oldfile" | "old" | "old_file" => Some(Category::OldFile),
            "large" | "big" | "giga" => Some(Category::Large),
            "recyclebin" | "recycle" | "papelera" => Some(Category::RecycleBin),
            "winupdate" | "windowsupdate" | "wu" => Some(Category::WindowsUpdate),
            "windowsold" | "winold" => Some(Category::WindowsOld),
            "crashdump" | "crash" | "dumps" => Some(Category::CrashDump),
            "appcache" | "app" => Some(Category::AppCache),
            "devcache" | "dev" => Some(Category::DevCache),
            "aimodel" | "aimodels" | "models" | "ia" => Some(Category::AIModel),
            "virtualdisk" | "vhdx" | "vm" | "docker" | "wsl" => Some(Category::VirtualDisk),
            "custom" => Some(Category::Custom),
            _ => None,
        }
    }
    pub fn all() -> Vec<Category> {
        vec![
            Category::Temp, Category::Cache, Category::Residue, Category::Log,
            Category::OldFile, Category::Large, Category::RecycleBin,
            Category::WindowsUpdate, Category::WindowsOld, Category::CrashDump,
            Category::AppCache, Category::DevCache, Category::AIModel, Category::VirtualDisk,
        ]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeleteMode {
    RecycleBin,
    Permanent,
}

impl DeleteMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            DeleteMode::RecycleBin => "recyclebin",
            DeleteMode::Permanent => "permanent",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanItem {
    pub path: PathBuf,
    pub size: u64,
    pub modified: SystemTime,
    pub category: Category,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanItem {
    pub item: ScanItem,
    pub selected: bool,
    pub action: DeleteMode,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    pub items: Vec<PlanItem>,
    pub generated_at: DateTime<Utc>,
    pub roots: Vec<PathBuf>,
}

impl Plan {
    pub fn total_size(&self) -> u64 {
        self.items.iter().filter(|i| i.selected).map(|i| i.item.size).sum()
    }
    pub fn selected_count(&self) -> usize {
        self.items.iter().filter(|i| i.selected).count()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionRecord {
    pub path: PathBuf,
    pub size: u64,
    pub category: Category,
    pub mode: DeleteMode,
    pub success: bool,
    pub error: Option<String>,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionReport {
    pub started_at: DateTime<Utc>,
    pub finished_at: DateTime<Utc>,
    pub records: Vec<ExecutionRecord>,
    pub total_freed: u64,
    pub succeeded: usize,
    pub failed: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleSet {
    pub include_paths: Vec<PathBuf>,
    pub exclude_paths: Vec<PathBuf>,
    pub exclude_extensions: Vec<String>,
    pub min_age_days: u32,
    pub min_size_bytes: u64,
    pub max_size_bytes: Option<u64>,
    pub large_min_bytes: u64,
    pub large_scan_paths: Vec<PathBuf>,
    pub selected_drives: Vec<String>,
    pub ui_dark_mode: bool,
    pub ui_language: String,
    pub categories: Vec<Category>,
    pub permanent_delete: bool,
}

impl Default for RuleSet {
    fn default() -> Self {
        let mut roots = vec![std::env::temp_dir()];
        let home = std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(PathBuf::from);

        // OS-specific roots
        #[cfg(target_os = "windows")]
        {
            if let Some(local) = std::env::var_os("LOCALAPPDATA") {
                let p = PathBuf::from(&local);
                roots.push(p.join("Temp"));
                roots.push(p.join("CrashDumps"));
                for sub in [
                    "Google\\Chrome\\User Data\\Default\\Cache",
                    "Google\\Chrome\\User Data\\Default\\Code Cache",
                    "Microsoft\\Edge\\User Data\\Default\\Cache",
                    "Mozilla\\Firefox\\Profiles",
                    "Spotify\\Storage",
                    "Discord\\Cache",
                    "Microsoft\\Teams\\Cache",
                    "Slack\\Cache",
                    "Telegram Desktop\\tdata",
                ] {
                    let q = p.join(sub);
                    if q.exists() { roots.push(q); }
                }
            }
            if let Some(win) = std::env::var_os("WINDIR") {
                let p = PathBuf::from(&win);
                roots.push(p.join("Temp"));
                roots.push(p.join("Prefetch"));
                roots.push(p.join("SoftwareDistribution\\Download"));
                roots.push(p.join("Minidump"));
                roots.push(p.join("LiveKernelReports"));
            }
            if PathBuf::from("C:\\Windows.old").exists() {
                roots.push(PathBuf::from("C:\\Windows.old"));
            }
            if let Some(pd) = std::env::var_os("PROGRAMDATA") {
                let p = PathBuf::from(pd);
                roots.push(p.join("Microsoft\\Windows\\WER"));
            }
            // Docker / WSL virtual disks
            if let Some(local) = std::env::var_os("LOCALAPPDATA") {
                let p = PathBuf::from(&local);
                let docker_data = p.join("Docker\\wsl\\data");
                if docker_data.exists() { roots.push(docker_data); }
                let packages = p.join("Packages");
                if packages.exists() {
                    if let Ok(rd) = std::fs::read_dir(&packages) {
                        for e in rd.flatten() {
                            let name = e.file_name().to_string_lossy().to_lowercase();
                            if name.contains("canonicalgroup") || name.contains("ubuntu") || name.contains("debian") || name.contains("kali") {
                                let localstate = e.path().join("LocalState");
                                if localstate.exists() { roots.push(localstate); }
                            }
                        }
                    }
                }
            }
        }

        #[cfg(target_os = "linux")]
        {
            roots.push(PathBuf::from("/tmp"));
            roots.push(PathBuf::from("/var/tmp"));
            if let Some(h) = &home {
                roots.push(h.join(".cache"));
                roots.push(h.join(".local/share/Trash"));
                roots.push(h.join(".npm/_cacache"));
                roots.push(h.join(".cargo/registry/cache"));
                roots.push(h.join(".gradle/caches"));
            }
            // browser caches
            if let Some(h) = &home {
                for sub in [
                    ".config/google-chrome/Default/Cache",
                    ".config/chromium/Default/Cache",
                    ".mozilla/firefox",
                    ".cache/mozilla/firefox",
                ] {
                    let q = h.join(sub);
                    if q.exists() { roots.push(q); }
                }
            }
            roots.push(PathBuf::from("/var/log"));
        }

        #[cfg(target_os = "macos")]
        {
            roots.push(PathBuf::from("/tmp"));
            if let Some(h) = &home {
                roots.push(h.join("Library/Caches"));
                roots.push(h.join(".Trash"));
                roots.push(h.join(".cache"));
                roots.push(h.join(".npm/_cacache"));
                roots.push(h.join(".cargo/registry/cache"));
                // browser caches
                for sub in [
                    "Library/Caches/Google/Chrome/Default/Cache",
                    "Library/Caches/Firefox/Profiles",
                    "Library/Caches/com.apple.Safari",
                ] {
                    let q = h.join(sub);
                    if q.exists() { roots.push(q); }
                }
            }
            roots.push(PathBuf::from("/var/log"));
        }

        // Dev caches + AI model caches (cross-platform, under home)
        if let Some(h) = &home {
            for sub in [
                ".npm/_cacache",
                ".cache",
                ".cargo/registry/cache",
                ".gradle/caches",
                ".m2/repository",
                ".nuget/packages",
                ".yarn/cache",
                ".pnpm-store",
                ".ollama",
                ".ollama/models",
                ".lmstudio",
                ".lmstudio/models",
                ".cache/huggingface/hub",
                ".cache/lm-studio/models",
                ".cache/torch",
                ".cache/gpt4all",
                ".jan/models",
                ".cache/llama.cpp",
                ".llama",
                ".llama-models",
                ".llamacpp",
            ] {
                let q = h.join(sub);
                if q.exists() { roots.push(q); }
            }
            // Shallow scan for AI framework install dirs (home only)
            if let Ok(rd) = std::fs::read_dir(&h) {
                for e in rd.flatten() {
                    let name = e.file_name().to_string_lossy().to_lowercase();
                    if name.contains("comfyui") || name.contains("stable-diffusion") || name.contains("sd-webui")
                        || name.contains("text-generation-webui") || name.contains("kobold") || name.contains("oobabooga")
                        || name.contains("invokeai") || name.contains("automatic1111") || name.contains("llama")
                        || name.contains("llamacpp") || name.contains("llama.cpp") || name.contains("lmstudio")
                        || name.contains("lm-studio") {
                        if e.path().is_dir() { roots.push(e.path()); }
                    }
                }
            }
        }
        Self {
            include_paths: roots,
            exclude_paths: vec![],
            exclude_extensions: vec![],
            min_age_days: 0,
            min_size_bytes: 0,
            max_size_bytes: None,
            large_min_bytes: 1024 * 1024 * 1024, // 1 GB
            large_scan_paths: default_large_scan_paths(),
            selected_drives: default_selected_drives(),
            ui_dark_mode: true,
            ui_language: "es".to_string(),
            categories: Category::all(),
            permanent_delete: false,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("trash error: {0}")]
    Trash(String),
    #[error("config error: {0}")]
    Config(String),
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("{0}")]
    Other(String),
}
pub type Result<T> = std::result::Result<T, CoreError>;

fn default_selected_drives() -> Vec<String> {
    // Default to the system drive
    #[cfg(target_os = "windows")]
    {
        vec!["C:\\".to_string()]
    }
    #[cfg(target_os = "linux")]
    {
        vec!["/".to_string()]
    }
    #[cfg(target_os = "macos")]
    {
        vec!["/".to_string()]
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    {
        vec!["/".to_string()]
    }
}

fn default_large_scan_paths() -> Vec<PathBuf> {
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    let mut out = Vec::new();
    for name in ["Downloads", "Documents", "Desktop", "Videos", "Pictures", "Music"] {
        let p = home.join(name);
        if p.exists() { out.push(p); }
    }
    out
}
