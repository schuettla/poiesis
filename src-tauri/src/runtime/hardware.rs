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
        avx2: std::arch::is_x86_feature_detected!("avx2"),
        avx512: std::arch::is_x86_feature_detected!("avx512f"),
    }
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
fn detect_gpus_wmi() -> Vec<GpuInfo> {
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

#[cfg(not(target_os = "windows"))]
fn detect_gpus_wmi() -> Vec<GpuInfo> {
    Vec::new()
}

/// Merge the accurate NVIDIA probe with the WMI list, preferring NVIDIA-SMI data
/// and de-duplicating by name.
fn merge_gpus(nvidia: Vec<GpuInfo>, wmi: Vec<GpuInfo>) -> Vec<GpuInfo> {
    let mut result = nvidia;
    for g in wmi {
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
    let gpus = merge_gpus(detect_nvidia(), detect_gpus_wmi());

    HardwareProfile { cpu, ram_mb, gpus }
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
}
