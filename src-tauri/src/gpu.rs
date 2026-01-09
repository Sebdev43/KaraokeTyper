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
}

/// Check if GPU features were enabled at compile time
fn is_gpu_enabled() -> bool {
    cfg!(feature = "gpu-vulkan") || cfg!(feature = "gpu-metal")
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

    // GPU acceleration is only available if compiled with gpu-vulkan feature
    let gpu_available = is_gpu_enabled() && has_vulkan;

    GpuDetectionResult {
        available: gpu_available,
        backend: if gpu_available {
            "Vulkan".to_string()
        } else {
            "CPU".to_string()
        },
        device_name,
    }
}

#[cfg(target_os = "macos")]
pub fn detect_gpu() -> GpuDetectionResult {
    let mut device_name = None;

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
                        break;
                    }
                }
            }
        }
    }

    // GPU acceleration is only available if compiled with gpu-metal feature
    let gpu_available = is_gpu_enabled();

    GpuDetectionResult {
        available: gpu_available,
        backend: if gpu_available {
            "Metal".to_string()
        } else {
            "CPU".to_string()
        },
        device_name,
    }
}

#[cfg(target_os = "windows")]
pub fn detect_gpu() -> GpuDetectionResult {
    let mut device_name = None;
    let mut has_gpu = false;

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

    // GPU acceleration is only available if compiled with gpu-vulkan feature
    let gpu_available = is_gpu_enabled() && has_gpu;

    GpuDetectionResult {
        available: gpu_available,
        backend: if gpu_available {
            "Vulkan".to_string()
        } else {
            "CPU".to_string()
        },
        device_name,
    }
}
