//! GPU detection module for displaying acceleration status in Settings
//!
//! Detects GPU availability and reports the backend being used (Metal/Vulkan/CPU).
//! GPU acceleration requires compiling with `--features gpu-vulkan` (Linux/Windows)
//! or `--features gpu-metal` (macOS).

use std::process::Command;

pub struct GpuDetectionResult {
    pub available: bool,
    pub backend: String,
    pub device_name: Option<String>,
    pub vram_mb: Option<u32>,
}

/// VRAM requirements per whisper model (approximate, in MB)
pub fn model_vram_mb(model_name: &str) -> u32 {
    let name = model_name.to_lowercase();
    if name.contains("large") {
        3000
    } else if name.contains("medium") {
        1500
    } else if name.contains("small") {
        700
    } else if name.contains("base") {
        300
    } else {
        // tiny or unknown
        150
    }
}

/// Returns the largest recommended model for the given VRAM (with safety margin)
pub fn recommended_model_for_vram(vram_mb: u32) -> &'static str {
    // Keep 30% margin to avoid OOM
    let usable = (vram_mb as f64 * 0.7) as u32;
    if usable >= 3000 {
        "ggml-large-v3"
    } else if usable >= 1500 {
        "ggml-medium"
    } else if usable >= 700 {
        "ggml-small"
    } else if usable >= 300 {
        "ggml-base"
    } else {
        "ggml-tiny"
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
    for entry in std::fs::read_dir("/sys/class/drm").into_iter().flatten() {
        if let Ok(entry) = entry {
            let vram_path = entry.path().join("device/mem_info_vram_total");
            if let Ok(contents) = std::fs::read_to_string(&vram_path) {
                if let Ok(bytes) = contents.trim().parse::<u64>() {
                    return Some((bytes / (1024 * 1024)).min(u32::MAX as u64) as u32);
                }
            }
        }
    }

    None
}

#[cfg(target_os = "linux")]
pub fn detect_gpu() -> GpuDetectionResult {
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
                if lower.contains("vga") || lower.contains("3d") || lower.contains("display") {
                    if lower.contains("nvidia")
                        || lower.contains("amd")
                        || lower.contains("intel")
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
pub fn detect_gpu() -> GpuDetectionResult {
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
pub fn detect_gpu() -> GpuDetectionResult {
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
    fn test_model_vram_mb() {
        assert_eq!(model_vram_mb("ggml-tiny"), 150);
        assert_eq!(model_vram_mb("ggml-base"), 300);
        assert_eq!(model_vram_mb("ggml-small"), 700);
        assert_eq!(model_vram_mb("ggml-medium"), 1500);
        assert_eq!(model_vram_mb("ggml-large-v3"), 3000);
        assert_eq!(model_vram_mb("unknown-model"), 150);
    }

    #[test]
    fn test_recommended_model_for_vram() {
        assert_eq!(recommended_model_for_vram(2048), "ggml-small");
        assert_eq!(recommended_model_for_vram(4096), "ggml-medium");
        assert_eq!(recommended_model_for_vram(8192), "ggml-large-v3");
        assert_eq!(recommended_model_for_vram(1024), "ggml-small");
        assert_eq!(recommended_model_for_vram(512), "ggml-base");
        assert_eq!(recommended_model_for_vram(256), "ggml-tiny");
        assert_eq!(recommended_model_for_vram(100), "ggml-tiny");
    }
}
