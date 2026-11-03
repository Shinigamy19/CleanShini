use crate::model::Result;

/// Vacía la papelera de reciclaje / basura del sistema.
pub fn empty_recycle_bin() -> Result<()> {
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::UI::Shell::SHEmptyRecycleBinW;
        use windows::core::PCWSTR;
        unsafe {
            let flags = 0x00000001u32 | 0x00000002 | 0x00000004;
            let _ = SHEmptyRecycleBinW(Some(HWND::default()), PCWSTR::null(), flags);
        }
        Ok(())
    }
    #[cfg(target_os = "linux")]
    {
        // Try gio trash --empty, then trash-empty
        let _ = std::process::Command::new("gio").args(["trash", "--empty"]).status();
        let _ = std::process::Command::new("trash-empty").status();
        Ok(())
    }
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
        if let Some(h) = home {
            let trash = h.join(".Trash");
            if trash.exists() {
                let _ = std::fs::remove_dir_all(&trash);
                let _ = std::fs::create_dir_all(&trash);
            }
        }
        Ok(())
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    {
        Ok(())
    }
}

/// Información de una unidad de disco.
#[derive(Debug, Clone)]
pub struct DriveInfo {
    pub root: String,
    pub label: String,
}

/// Lista las unidades de disco disponibles en el sistema.
pub fn list_drives() -> Vec<DriveInfo> {
    let mut out = Vec::new();
    #[cfg(target_os = "windows")]
    {
        for letter in b'A'..=b'Z' {
            let root = format!("{}:\\", letter as char);
            if std::path::Path::new(&root).exists() {
                out.push(DriveInfo { root: root.clone(), label: format!("Unidad {}", letter as char) });
            }
        }
    }
    #[cfg(target_os = "linux")]
    {
        // mount points from /proc/mounts (only real filesystems)
        if let Ok(content) = std::fs::read_to_string("/proc/mounts") {
            for line in content.lines() {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 2 {
                    let mp = parts[1];
                    if mp == "/" || mp.starts_with("/home") || mp.starts_with("/mnt") || mp.starts_with("/media") {
                        if std::path::Path::new(mp).exists() && !out.iter().any(|d: &DriveInfo| d.root == mp) {
                            out.push(DriveInfo { root: mp.to_string(), label: mp.to_string() });
                        }
                    }
                }
            }
        }
        if out.is_empty() {
            out.push(DriveInfo { root: "/".to_string(), label: "/".to_string() });
        }
    }
    #[cfg(target_os = "macos")]
    {
        out.push(DriveInfo { root: "/".to_string(), label: "Macintosh HD".to_string() });
        if let Ok(rd) = std::fs::read_dir("/Volumes") {
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    out.push(DriveInfo { root: p.to_string_lossy().to_string(), label: e.file_name().to_string_lossy().to_string() });
                }
            }
        }
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    {
        out.push(DriveInfo { root: "/".to_string(), label: "/".to_string() });
    }
    out
}

#[derive(Debug, Clone)]
pub struct SystemCmdResult {
    pub name: String,
    pub success: bool,
    pub output: String,
}

pub fn run_system_cleanup(dism: bool, cleanmgr: bool, hibernation_off: bool, shadow_delete: bool) -> Vec<SystemCmdResult> {
    let mut results = Vec::new();
    #[cfg(target_os = "windows")]
    {
        if dism {
            results.push(run_cmd("DISM component cleanup", "dism.exe", &["/Online", "/Cleanup-Image", "/StartComponentCleanup"]));
        }
        if cleanmgr {
            results.push(run_cmd("Cleanmgr (sagerun:1)", "cleanmgr.exe", &["/sagerun:1"]));
        }
        if hibernation_off {
            results.push(run_cmd("Deshabilitar hibernación", "powercfg.exe", &["/h", "off"]));
        }
        if shadow_delete {
            results.push(run_cmd("Eliminar shadow copies", "vssadmin.exe", &["delete", "shadows", "/all", "/quiet"]));
        }
    }
    #[cfg(target_os = "linux")]
    {
        if dism {
            results.push(run_cmd("apt autoremove", "sudo", &["apt-get", "autoremove", "-y"]));
            results.push(run_cmd("apt clean", "sudo", &["apt-get", "clean"]));
        }
        if cleanmgr {
            results.push(run_cmd("journalctl vacuum 7d", "sudo", &["journalctl", "--vacuum-time=7d"]));
        }
        if hibernation_off {
            // disable swap (hibernation) - best effort
            results.push(run_cmd("Disable swap", "sudo", &["swapoff", "-a"]));
        }
        if shadow_delete {
            // no-op on linux
        }
    }
    #[cfg(target_os = "macos")]
    {
        if dism {
            results.push(run_cmd("brew cleanup", "brew", &["cleanup", "--prune=all"]));
        }
        if cleanmgr {
            results.push(run_cmd("periodic maintenance", "sudo", &["periodic", "daily", "weekly", "monthly"]));
        }
        if hibernation_off {
            results.push(run_cmd("Disable hibernation", "sudo", &["pmset", "-a", "hibernatemode", "0"]));
        }
        if shadow_delete {
            // no-op on macos
        }
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    {
        let _ = (dism, cleanmgr, hibernation_off, shadow_delete);
    }
    results
}

#[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
fn run_cmd(name: &str, prog: &str, args: &[&str]) -> SystemCmdResult {
    use std::process::Command;
    let out = Command::new(prog).args(args).output();
    match out {
        Ok(o) => SystemCmdResult {
            name: name.to_string(),
            success: o.status.success(),
            output: String::from_utf8_lossy(&o.stdout).to_string() + &String::from_utf8_lossy(&o.stderr),
        },
        Err(e) => SystemCmdResult { name: name.to_string(), success: false, output: e.to_string() },
    }
}
