//! GPU detection module for displaying acceleration status in Settings
//!
//! Detects GPU availability and reports the backend being used (Metal/Vulkan/CPU).
//! GPU acceleration requires compiling with `--features gpu-vulkan` (Linux/Windows)
//! or `--features gpu-metal` (macOS).

use std::process::Command;
use std::sync::OnceLock;

#[derive(Clone)]
pub struct GpuDetectionResult {
    pub available: bool,
    pub backend: String,
    pub device_name: Option<String>,
    pub vram_mb: Option<u32>,
}

/// Cached result of the (expensive) hardware probe below.
static GPU_DETECTION_CACHE: OnceLock<GpuDetectionResult> = OnceLock::new();

/// GPU detection, memoized for the process lifetime.
///
/// The platform probes shell out to 2-4 external binaries (`vulkaninfo`,
/// `lspci`, `nvidia-smi`, `system_profiler`, `wmic`…). The result depends only
/// on the hardware and compile-time GPU features, neither of which change at
/// runtime, so running it once and caching is both safe and a clear win for the
/// Settings UI which may call this repeatedly.
#[must_use]
pub fn detect_gpu() -> GpuDetectionResult {
    GPU_DETECTION_CACHE.get_or_init(detect_gpu_uncached).clone()
}

/// Rough VRAM a Whisper model needs, in MB: the GGUF file plus room for the
/// compute buffers, about half as much again.
///
/// The figures dropped when the catalogue moved to quantised GGUF — large-v3 is
/// 1.2 GB on disk now, not 3.1 — so a card that used to be told it could only run
/// medium runs large-v3 comfortably. Matching is on substrings because the caller
/// passes whatever is in the settings, including names from older versions.
#[allow(dead_code)]
pub fn model_vram_mb(model_name: &str) -> u32 {
    let name = model_name.to_lowercase();
    // Before "large": the turbo build carries both words and is much smaller.
    if name.contains("turbo") {
        1400
    } else if name.contains("large") {
        1800
    } else if name.contains("medium") {
        1300
    } else if name.contains("small") {
        450
    } else if name.contains("base") {
        150
    } else {
        // tiny or unknown
        100
    }
}

/// Returns the largest recommended model for the given VRAM (with safety margin)
pub fn recommended_model_for_vram(vram_mb: u32) -> &'static str {
    // Keep 30% margin to avoid OOM
    let usable = (vram_mb as f64 * 0.7) as u32;
    if usable >= model_vram_mb("whisper-large-v3") {
        "whisper-large-v3"
    } else if usable >= model_vram_mb("whisper-large-v3-turbo") {
        "whisper-large-v3-turbo"
    } else if usable >= model_vram_mb("whisper-medium") {
        "whisper-medium"
    } else if usable >= model_vram_mb("whisper-small") {
        "whisper-small"
    } else if usable >= model_vram_mb("whisper-base") {
        "whisper-base"
    } else {
        "whisper-tiny"
    }
}

/// Check if GPU features were enabled at compile time
fn is_gpu_enabled() -> bool {
    cfg!(feature = "gpu-vulkan") || cfg!(feature = "gpu-metal")
}

#[cfg(target_os = "linux")]
fn detect_vram_linux() -> Option<u32> {
    // Try nvidia-smi first (NVIDIA GPUs)
    if let Ok(output) = Command::new("nvidia-smi")
        .args(["--query-gpu=memory.total", "--format=csv,noheader,nounits"])
        .output()
    {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if let Some(first_line) = stdout.trim().lines().next() {
                if let Ok(mb) = first_line.trim().parse::<u32>() {
                    return Some(mb);
                }
            }
        }
    }

    // Try AMD sysfs (AMD GPUs)
    for entry in std::fs::read_dir("/sys/class/drm")
        .into_iter()
        .flatten()
        .flatten()
    {
        let vram_path = entry.path().join("device/mem_info_vram_total");
        if let Ok(contents) = std::fs::read_to_string(&vram_path) {
            if let Ok(bytes) = contents.trim().parse::<u64>() {
                return Some((bytes / (1024 * 1024)).min(u32::MAX as u64) as u32);
            }
        }
    }

    None
}

#[cfg(target_os = "linux")]
fn detect_gpu_uncached() -> GpuDetectionResult {
    let mut device_name = None;
    let mut has_vulkan = false;

    // Try vulkaninfo first for accurate Vulkan detection
    if let Ok(output) = Command::new("vulkaninfo").arg("--summary").output() {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            has_vulkan = stdout.contains("deviceName");

            for line in stdout.lines() {
                if line.contains("deviceName") {
                    if let Some(name) = line.split('=').nth(1) {
                        device_name = Some(name.trim().to_string());
                        break;
                    }
                }
            }
        }
    }

    // Fallback to lspci for GPU name detection
    if device_name.is_none() {
        if let Ok(output) = Command::new("lspci").output() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                let lower = line.to_lowercase();
                if (lower.contains("vga") || lower.contains("3d") || lower.contains("display"))
                    && (lower.contains("nvidia")
                        || lower.contains("amd")
                        || lower.contains("intel"))
                {
                    if let Some(name_part) = line.split(':').nth(2) {
                        device_name = Some(name_part.trim().to_string());
                        has_vulkan = true;
                        break;
                    }
                }
            }
        }
    }

    let gpu_available = is_gpu_enabled() && has_vulkan;
    let vram_mb = detect_vram_linux();

    GpuDetectionResult {
        available: gpu_available,
        backend: if gpu_available {
            "Vulkan".to_string()
        } else {
            "CPU".to_string()
        },
        device_name,
        vram_mb,
    }
}

#[cfg(target_os = "macos")]
fn detect_gpu_uncached() -> GpuDetectionResult {
    let mut device_name = None;
    let mut vram_mb: Option<u32> = None;

    if let Ok(output) = Command::new("system_profiler")
        .arg("SPDisplaysDataType")
        .output()
    {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with("Chipset Model:") {
                    if let Some(name) = trimmed.strip_prefix("Chipset Model:") {
                        device_name = Some(name.trim().to_string());
                    }
                }
                if trimmed.starts_with("VRAM") {
                    // Parse "VRAM (Total): 8 GB" or "VRAM (Dynamic, Max): 16384 MB"
                    if let Some(val_str) = trimmed.split(':').nth(1) {
                        let val_str = val_str.trim();
                        if val_str.ends_with("GB") {
                            if let Ok(gb) = val_str.trim_end_matches("GB").trim().parse::<u32>() {
                                vram_mb = gb.checked_mul(1024);
                            }
                        } else if val_str.ends_with("MB") {
                            if let Ok(mb) = val_str.trim_end_matches("MB").trim().parse::<u32>() {
                                vram_mb = Some(mb);
                            }
                        }
                    }
                }
            }
        }
    }

    let gpu_available = is_gpu_enabled();

    GpuDetectionResult {
        available: gpu_available,
        backend: if gpu_available {
            "Metal".to_string()
        } else {
            "CPU".to_string()
        },
        device_name,
        vram_mb,
    }
}

#[cfg(target_os = "windows")]
fn detect_gpu_uncached() -> GpuDetectionResult {
    let mut device_name = None;
    let mut has_gpu = false;
    let mut vram_mb: Option<u32> = None;

    if let Ok(output) = Command::new("wmic")
        .args(["path", "win32_VideoController", "get", "name"])
        .output()
    {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines().skip(1) {
                let trimmed = line.trim();
                if !trimmed.is_empty() {
                    device_name = Some(trimmed.to_string());
                    has_gpu = true;
                    break;
                }
            }
        }
    }

    // Get VRAM via AdapterRAM (returns bytes)
    if let Ok(output) = Command::new("wmic")
        .args(["path", "win32_VideoController", "get", "AdapterRAM"])
        .output()
    {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines().skip(1) {
                let trimmed = line.trim();
                if let Ok(bytes) = trimmed.parse::<u64>() {
                    vram_mb = Some((bytes / (1024 * 1024)).min(u32::MAX as u64) as u32);
                    break;
                }
            }
        }
    }

    let gpu_available = is_gpu_enabled() && has_gpu;

    GpuDetectionResult {
        available: gpu_available,
        backend: if gpu_available {
            "Vulkan".to_string()
        } else {
            "CPU".to_string()
        },
        device_name,
        vram_mb,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vram_needs_follow_the_quantised_gguf_sizes() {
        // Roughly the file plus its compute buffers; what matters is the order and
        // that large-v3 is no longer sized for the 3.1 GB GGML it used to be.
        assert_eq!(model_vram_mb("whisper-tiny"), 100);
        assert_eq!(model_vram_mb("whisper-base"), 150);
        assert_eq!(model_vram_mb("whisper-small"), 450);
        assert_eq!(model_vram_mb("whisper-medium"), 1300);
        assert_eq!(model_vram_mb("whisper-large-v3"), 1800);
        // Both words are in the name; the turbo build is the smaller one.
        assert_eq!(model_vram_mb("whisper-large-v3-turbo"), 1400);
        // A name left over from an older version still resolves.
        assert_eq!(model_vram_mb("ggml-large-v3"), 1800);
        assert_eq!(model_vram_mb("unknown-model"), 100);
    }

    #[test]
    fn a_card_is_recommended_the_largest_model_that_fits() {
        // 2 GB card: 1.4 GB usable — large-v3 (1.8) does not fit, turbo (1.4) does.
        assert_eq!(recommended_model_for_vram(2048), "whisper-large-v3-turbo");
        // 4 GB and 8 GB both clear large-v3; the old table stopped at medium for 4 GB.
        assert_eq!(recommended_model_for_vram(4096), "whisper-large-v3");
        assert_eq!(recommended_model_for_vram(8192), "whisper-large-v3");
        // 1 GB card: 716 MB usable — medium is out of reach, small fits.
        assert_eq!(recommended_model_for_vram(1024), "whisper-small");
        assert_eq!(recommended_model_for_vram(256), "whisper-base");
        assert_eq!(recommended_model_for_vram(100), "whisper-tiny");
        // Every recommendation must be a model the catalogue actually ships.
        for vram in [512, 1024, 2048, 4096, 8192, 24576] {
            let name = recommended_model_for_vram(vram);
            assert!(
                crate::catalog::models().iter().any(|m| m.name == name),
                "{name} is not in the catalogue"
            );
        }
    }
}
