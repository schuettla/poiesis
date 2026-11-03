//! Hardware survey (PRD §7.3.3 step 1): GPU vendor/model + VRAM, system RAM,
//! and CPU instruction-set support. Feeds both runtime selection (§7.3.2) and
//! the marketplace's "Runs great / slowly / Won't fit" classifier (MKT-4).

use serde::{Deserialize, Serialize};
use std::process::Command;
use sysinfo::System;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GpuVendor {
    Nvidia,
    Amd,
    Intel,
    /// Apple Silicon's built-in GPU, which shares system memory (macOS only).
    Apple,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuInfo {
    pub vendor: GpuVendor,
    pub name: String,
    /// Total dedicated video memory in MB, when it can be determined.
    pub vram_mb: Option<u64>,
    pub driver_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CpuInfo {
    pub brand: String,
    pub physical_cores: usize,
    pub avx2: bool,
    pub avx512: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HardwareProfile {
    pub cpu: CpuInfo,
    /// Total system RAM in MB.
    pub ram_mb: u64,
    /// Detected GPUs (discrete first). Empty on CPU-only machines.
    pub gpus: Vec<GpuInfo>,
}

impl HardwareProfile {
    /// The GPU the runtime selector should target: the first discrete GPU, or
    /// the first GPU of any kind if none are clearly discrete.
    pub fn primary_gpu(&self) -> Option<&GpuInfo> {
        self.gpus
            .iter()
            .find(|g| g.vendor != GpuVendor::Unknown)
            .or_else(|| self.gpus.first())
    }

    /// VRAM on the primary GPU, or 0 on a CPU-only machine.
    pub fn vram_mb(&self) -> u64 {
        self.primary_gpu().and_then(|g| g.vram_mb).unwrap_or(0)
    }
}

/// Hardware-fit verdict for a model on this machine (MKT-4). Lives here rather
/// than in the language-model marketplace because it is the same question for
/// any weights at all — a diffusion checkpoint has to fit on the card just as a
/// GGUF does, and both catalogs answer it with this one classifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Fit {
    /// Fits comfortably in VRAM — fast.
    Great,
    /// Fits in RAM but not VRAM — runs on CPU, slower.
    Slow,
    /// Too large for available memory.
    WontFit,
}

impl Fit {
    // The frontend renders its own localized labels; kept for tests/logging.
    #[allow(dead_code)]
    pub fn label(&self) -> &'static str {
        match self {
            Fit::Great => "Runs great on your PC",
            Fit::Slow => "Runs slowly",
            Fit::WontFit => "Won't fit",
        }
    }
}

/// Classify how weights of `size_mb` will run on `hw`. Heuristic: they need
/// roughly their own size plus ~20% headroom. If that fits in GPU VRAM it runs
/// great; else if it fits in system RAM it runs (slowly) on CPU; otherwise it
/// won't fit.
pub fn classify_fit(size_mb: u64, hw: &HardwareProfile) -> Fit {
    let needed = (size_mb as f64 * 1.2) as u64;
    if hw.vram_mb() >= needed {
        Fit::Great
    } else if hw.ram_mb >= needed + 2048 {
        // Leave ~2 GB for the OS + app shell.
        Fit::Slow
    } else {
        Fit::WontFit
    }
}

fn detect_cpu(sys: &System) -> CpuInfo {
    let brand = sys
        .cpus()
        .first()
        .map(|c| c.brand().trim().to_string())
        .unwrap_or_else(|| "Unknown CPU".to_string());
    CpuInfo {
        brand,
        physical_cores: sys.physical_core_count().unwrap_or(0),
        // Runtime ISA detection — picks the right CPU build variant (§7.3.2).
        avx2: x86_feature("avx2"),
        avx512: x86_feature("avx512f"),
    }
}

/// `is_x86_feature_detected!` only exists on x86 targets; Apple Silicon has
/// neither feature.
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
fn x86_feature(name: &str) -> bool {
    match name {
        "avx2" => std::arch::is_x86_feature_detected!("avx2"),
        "avx512f" => std::arch::is_x86_feature_detected!("avx512f"),
        _ => false,
    }
}

#[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
fn x86_feature(_name: &str) -> bool {
    false
}

fn vendor_from_name(name: &str) -> GpuVendor {
    let n = name.to_ascii_lowercase();
    if n.contains("nvidia") || n.contains("geforce") || n.contains("quadro") || n.contains("rtx")
        || n.contains("gtx") || n.contains("tesla")
    {
        GpuVendor::Nvidia
    } else if n.contains("amd") || n.contains("radeon") || n.contains("ryzen") {
        GpuVendor::Amd
    } else if n.contains("intel") || n.contains("arc") || n.contains("iris") || n.contains("uhd") {
        GpuVendor::Intel
    } else {
        GpuVendor::Unknown
    }
}

/// Accurate NVIDIA probe via `nvidia-smi` (present with the driver). Returns the
/// detected NVIDIA GPUs, or an empty vec if the tool is absent.
fn detect_nvidia() -> Vec<GpuInfo> {
    let out = Command::new("nvidia-smi")
        .args([
            "--query-gpu=name,memory.total,driver_version",
            "--format=csv,noheader,nounits",
        ])
        .output();
    let Ok(out) = out else { return Vec::new() };
    if !out.status.success() {
        return Vec::new();
    }
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.split(',').map(|s| s.trim()).collect();
            if parts.len() < 3 {
                return None;
            }
            Some(GpuInfo {
                vendor: GpuVendor::Nvidia,
                name: parts[0].to_string(),
                vram_mb: parts[1].parse::<u64>().ok(),
                driver_version: Some(parts[2].to_string()),
            })
        })
        .collect()
}

/// Fallback GPU probe via Windows CIM/WMI. Note: `AdapterRAM` is a uint32 and
/// saturates at ~4 GB, so VRAM here is a lower bound for large cards — accurate
/// VRAM for NVIDIA comes from `detect_nvidia`.
#[cfg(target_os = "windows")]
fn detect_gpus_system(_ram_mb: u64) -> Vec<GpuInfo> {
    let script = "Get-CimInstance Win32_VideoController | \
        Select-Object Name, AdapterRAM, DriverVersion | ConvertTo-Json -Compress";
    let out = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .output();
    let Ok(out) = out else { return Vec::new() };
    let text = String::from_utf8_lossy(&out.stdout);
    let json: serde_json::Value = match serde_json::from_str(text.trim()) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let items = match &json {
        serde_json::Value::Array(a) => a.clone(),
        serde_json::Value::Object(_) => vec![json],
        _ => return Vec::new(),
    };
    items
        .into_iter()
        .filter_map(|item| {
            let name = item.get("Name")?.as_str()?.to_string();
            let vram_mb = item
                .get("AdapterRAM")
                .and_then(|v| v.as_u64())
                .map(|b| b / (1024 * 1024))
                .filter(|&mb| mb > 0);
            let driver_version = item
                .get("DriverVersion")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            Some(GpuInfo {
                vendor: vendor_from_name(&name),
                name,
                vram_mb,
                driver_version,
            })
        })
        .collect()
}

/// Linux fallback probe: the kernel's DRM devices. Enough for the selector to
/// pick Vulkan on AMD and Intel; NVIDIA names and VRAM come from `detect_nvidia`.
#[cfg(target_os = "linux")]
fn detect_gpus_system(_ram_mb: u64) -> Vec<GpuInfo> {
    drm_gpus(std::path::Path::new("/sys/class/drm"))
}

/// macOS: there is no probe to run. Apple Silicon always has a GPU, and it
/// shares system memory; macOS lets Metal use about two thirds of it, which is
/// what the fit classifier should compare against.
#[cfg(target_os = "macos")]
fn detect_gpus_system(ram_mb: u64) -> Vec<GpuInfo> {
    vec![GpuInfo {
        vendor: GpuVendor::Apple,
        name: "Apple GPU".to_string(),
        vram_mb: Some(ram_mb * 2 / 3),
        driver_version: None,
    }]
}

#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
fn detect_gpus_system(_ram_mb: u64) -> Vec<GpuInfo> {
    Vec::new()
}

/// Read GPUs from a `/sys/class/drm`-shaped directory: each `cardN/device`
/// holds the PCI `vendor` id, and amdgpu also reports `mem_info_vram_total`
/// in bytes. The `cardN-<connector>` entries beside them are outputs, not GPUs.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn drm_gpus(root: &std::path::Path) -> Vec<GpuInfo> {
    let Ok(entries) = std::fs::read_dir(root) else { return Vec::new() };
    let mut cards: Vec<_> = entries
        .flatten()
        .filter(|e| {
            let name = e.file_name();
            let name = name.to_string_lossy();
            name.strip_prefix("card")
                .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
        })
        .map(|e| e.path().join("device"))
        .collect();
    cards.sort();
    cards
        .into_iter()
        .filter_map(|dev| {
            let vendor = std::fs::read_to_string(dev.join("vendor")).ok()?;
            let (vendor, name) = match vendor.trim().to_ascii_lowercase().as_str() {
                "0x10de" => (GpuVendor::Nvidia, "NVIDIA GPU"),
                "0x1002" => (GpuVendor::Amd, "AMD GPU"),
                "0x8086" => (GpuVendor::Intel, "Intel GPU"),
                _ => return None,
            };
            let vram_mb = std::fs::read_to_string(dev.join("mem_info_vram_total"))
                .ok()
                .and_then(|s| s.trim().parse::<u64>().ok())
                .map(|b| b / (1024 * 1024))
                .filter(|&mb| mb > 0);
            Some(GpuInfo {
                vendor,
                name: name.to_string(),
                vram_mb,
                driver_version: None,
            })
        })
        .collect()
}

/// Merge the accurate NVIDIA probe with the OS's own GPU list, preferring
/// NVIDIA-SMI data and de-duplicating by name.
fn merge_gpus(nvidia: Vec<GpuInfo>, system: Vec<GpuInfo>) -> Vec<GpuInfo> {
    let mut result = nvidia;
    for g in system {
        let dup = result.iter().any(|r| {
            r.name.eq_ignore_ascii_case(&g.name)
                || (r.vendor == GpuVendor::Nvidia && g.vendor == GpuVendor::Nvidia)
        });
        if !dup {
            result.push(g);
        }
    }
    // Discrete (known vendor) first, integrated/unknown last.
    result.sort_by_key(|g| matches!(g.vendor, GpuVendor::Unknown | GpuVendor::Intel));
    result
}

/// Survey the machine. Cheap enough to call on demand; the result is cached by
/// the caller (first run) and re-surveyed when the user asks (§7.3.3).
pub fn detect_hardware() -> HardwareProfile {
    let mut sys = System::new();
    sys.refresh_memory();
    sys.refresh_cpu_all();

    let cpu = detect_cpu(&sys);
    let ram_mb = sys.total_memory() / (1024 * 1024);
    let gpus = merge_gpus(detect_nvidia(), detect_gpus_system(ram_mb));

    HardwareProfile { cpu, ram_mb, gpus }
}

/// The survey, taken once per run of the app. The machine does not change under
/// us, and the model listing asks on every refresh.
pub fn detect_hardware_cached() -> HardwareProfile {
    static PROFILE: std::sync::OnceLock<HardwareProfile> = std::sync::OnceLock::new();
    PROFILE.get_or_init(detect_hardware).clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::manifest::select_runtime;

    #[test]
    fn surveys_this_machine() {
        let profile = detect_hardware();
        println!("\n--- hardware profile ---\n{profile:#?}");
        let selection = select_runtime(&profile);
        println!("\n--- runtime selection ---\n{selection:#?}\n");
        assert!(profile.ram_mb > 0, "should detect some RAM");
    }

    #[test]
    fn reads_gpus_from_a_drm_tree() {
        let root = tempfile::tempdir().unwrap();
        let card = |name: &str, vendor: &str, vram: Option<&str>| {
            let dev = root.path().join(name).join("device");
            std::fs::create_dir_all(&dev).unwrap();
            std::fs::write(dev.join("vendor"), format!("{vendor}\n")).unwrap();
            if let Some(v) = vram {
                std::fs::write(dev.join("mem_info_vram_total"), v).unwrap();
            }
        };
        card("card0", "0x8086", None);
        card("card1", "0x1002", Some("17163091968\n"));
        // A connector, not a GPU: must be skipped even though it looks like one.
        card("card1-DP-1", "0x1002", None);
        card("card2", "0x1af4", None); // virtio: not a vendor we drive

        let gpus = drm_gpus(root.path());
        assert_eq!(gpus.len(), 2, "{gpus:#?}");
        assert_eq!(gpus[0].vendor, GpuVendor::Intel);
        assert_eq!(gpus[1].vendor, GpuVendor::Amd);
        assert_eq!(gpus[1].vram_mb, Some(16368));

        // Discrete AMD ahead of the integrated Intel, so Vulkan targets it.
        let merged = merge_gpus(Vec::new(), gpus);
        assert_eq!(merged[0].vendor, GpuVendor::Amd);
        assert_eq!(select_runtime(&HardwareProfile {
            cpu: CpuInfo { brand: String::new(), physical_cores: 8, avx2: true, avx512: false },
            ram_mb: 32768,
            gpus: merged,
        }).backend, crate::runtime::manifest::Backend::Vulkan);
    }

    #[test]
    fn missing_drm_dir_means_no_gpus() {
        assert!(drm_gpus(std::path::Path::new("/definitely/not/here")).is_empty());
    }
}
