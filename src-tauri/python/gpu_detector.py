#!/usr/bin/env python3
"""
Cross-Platform GPU Detection Module for KaraokeTyper

Detects available GPU hardware and recommends the best STT backend:
- NVIDIA: CUDA via ONNX Runtime CUDAExecutionProvider
- AMD: ROCm via ONNX Runtime MIGraphXExecutionProvider or ROCMExecutionProvider
- Apple: Metal via ONNX Runtime CoreMLExecutionProvider
- CPU: Fallback via ONNX Runtime CPUExecutionProvider

Output: JSON to stdout for Rust/Tauri integration
"""

import json
import sys
import os
import subprocess
import platform
from typing import Dict, List, Optional, Any


def detect_nvidia_gpu() -> Optional[Dict[str, Any]]:
    """Detect NVIDIA GPU using nvidia-smi or PyTorch CUDA"""
    gpu_info = None

    # Method 1: nvidia-smi (most reliable)
    try:
        result = subprocess.run(
            ["nvidia-smi", "--query-gpu=name,memory.total,compute_cap", "--format=csv,noheader,nounits"],
            capture_output=True,
            text=True,
            timeout=5
        )
        if result.returncode == 0 and result.stdout.strip():
            parts = result.stdout.strip().split(", ")
            if len(parts) >= 3:
                gpu_info = {
                    "vendor": "NVIDIA",
                    "name": parts[0].strip(),
                    "vram_mb": int(float(parts[1].strip())),
                    "compute_capability": parts[2].strip(),
                    "backend": "CUDAExecutionProvider",
                    "driver_version": None
                }

                # Get driver version
                driver_result = subprocess.run(
                    ["nvidia-smi", "--query-gpu=driver_version", "--format=csv,noheader"],
                    capture_output=True,
                    text=True,
                    timeout=5
                )
                if driver_result.returncode == 0:
                    gpu_info["driver_version"] = driver_result.stdout.strip()

                return gpu_info
    except (FileNotFoundError, subprocess.TimeoutExpired, Exception):
        pass

    # Method 2: PyTorch CUDA (fallback)
    try:
        import torch
        if torch.cuda.is_available():
            device_props = torch.cuda.get_device_properties(0)
            gpu_info = {
                "vendor": "NVIDIA",
                "name": device_props.name,
                "vram_mb": device_props.total_memory // (1024 * 1024),
                "compute_capability": f"{device_props.major}.{device_props.minor}",
                "backend": "CUDAExecutionProvider",
                "driver_version": None
            }
            return gpu_info
    except ImportError:
        pass
    except Exception:
        pass

    return None


def detect_amd_gpu() -> Optional[Dict[str, Any]]:
    """Detect AMD GPU using rocm-smi or system tools"""
    gpu_info = None

    # Method 1: rocm-smi (Linux with ROCm)
    try:
        result = subprocess.run(
            ["rocm-smi", "--showproductname", "--showmeminfo", "vram", "--json"],
            capture_output=True,
            text=True,
            timeout=5
        )
        if result.returncode == 0:
            data = json.loads(result.stdout)
            if "card0" in data:
                card = data["card0"]
                gpu_info = {
                    "vendor": "AMD",
                    "name": card.get("Card series", "AMD GPU"),
                    "vram_mb": int(card.get("VRAM Total Memory (B)", 0)) // (1024 * 1024),
                    "compute_capability": None,
                    "backend": "MIGraphXExecutionProvider",
                    "rocm_version": None
                }

                # Get ROCm version
                version_result = subprocess.run(
                    ["rocm-smi", "--showversion"],
                    capture_output=True,
                    text=True,
                    timeout=5
                )
                if version_result.returncode == 0:
                    for line in version_result.stdout.split("\n"):
                        if "ROCm" in line:
                            gpu_info["rocm_version"] = line.strip()
                            break

                return gpu_info
    except (FileNotFoundError, subprocess.TimeoutExpired, json.JSONDecodeError, Exception):
        pass

    # Method 2: Check /sys/class/drm for AMD GPUs (Linux)
    if platform.system() == "Linux":
        try:
            drm_path = "/sys/class/drm"
            if os.path.exists(drm_path):
                for card in os.listdir(drm_path):
                    if card.startswith("card") and not "-" in card:
                        vendor_path = os.path.join(drm_path, card, "device", "vendor")
                        if os.path.exists(vendor_path):
                            with open(vendor_path, "r") as f:
                                vendor_id = f.read().strip()
                                # 0x1002 = AMD vendor ID
                                if vendor_id == "0x1002":
                                    name_path = os.path.join(drm_path, card, "device", "product_name")
                                    gpu_name = "AMD GPU"
                                    if os.path.exists(name_path):
                                        with open(name_path, "r") as f:
                                            gpu_name = f.read().strip()

                                    # Try to get VRAM from mem_info_vram_total
                                    vram_path = os.path.join(drm_path, card, "device", "mem_info_vram_total")
                                    vram_mb = 0
                                    if os.path.exists(vram_path):
                                        with open(vram_path, "r") as f:
                                            vram_mb = int(f.read().strip()) // (1024 * 1024)

                                    gpu_info = {
                                        "vendor": "AMD",
                                        "name": gpu_name,
                                        "vram_mb": vram_mb,
                                        "compute_capability": None,
                                        "backend": "MIGraphXExecutionProvider",
                                        "rocm_installed": check_rocm_installed()
                                    }
                                    return gpu_info
        except Exception:
            pass

    # Method 3: lspci fallback (Linux)
    if platform.system() == "Linux":
        try:
            result = subprocess.run(
                ["lspci", "-nn"],
                capture_output=True,
                text=True,
                timeout=5
            )
            if result.returncode == 0:
                for line in result.stdout.split("\n"):
                    if "VGA" in line or "3D" in line:
                        # Check for AMD vendor ID [1002:]
                        if "[1002:" in line:
                            # Extract GPU name
                            parts = line.split(": ", 1)
                            gpu_name = parts[1] if len(parts) > 1 else "AMD GPU"
                            # Clean up the name
                            if "[" in gpu_name:
                                gpu_name = gpu_name.split("[")[0].strip()

                            gpu_info = {
                                "vendor": "AMD",
                                "name": gpu_name,
                                "vram_mb": 0,  # Can't determine from lspci
                                "compute_capability": None,
                                "backend": "MIGraphXExecutionProvider",
                                "rocm_installed": check_rocm_installed()
                            }
                            return gpu_info
        except (FileNotFoundError, subprocess.TimeoutExpired, Exception):
            pass

    # Method 4: PyTorch HIP/ROCm (if available)
    try:
        import torch
        if hasattr(torch, 'hip') and torch.hip.is_available():
            gpu_info = {
                "vendor": "AMD",
                "name": torch.hip.get_device_name(0),
                "vram_mb": 0,
                "compute_capability": None,
                "backend": "MIGraphXExecutionProvider",
                "rocm_installed": True
            }
            return gpu_info
    except ImportError:
        pass
    except Exception:
        pass

    return None


def detect_apple_gpu() -> Optional[Dict[str, Any]]:
    """Detect Apple Silicon GPU (M1/M2/M3/M4)"""
    if platform.system() != "Darwin":
        return None

    gpu_info = None

    # Method 1: system_profiler
    try:
        result = subprocess.run(
            ["system_profiler", "SPDisplaysDataType", "-json"],
            capture_output=True,
            text=True,
            timeout=10
        )
        if result.returncode == 0:
            data = json.loads(result.stdout)
            displays = data.get("SPDisplaysDataType", [])
            for display in displays:
                gpu_name = display.get("sppci_model", "")
                # Check if it's Apple Silicon
                if "Apple" in gpu_name or "M1" in gpu_name or "M2" in gpu_name or "M3" in gpu_name or "M4" in gpu_name:
                    vram = display.get("spdisplays_vram", "0")
                    # Parse VRAM (e.g., "8 GB" -> 8192 MB)
                    vram_mb = 0
                    if isinstance(vram, str):
                        parts = vram.split()
                        if parts:
                            try:
                                vram_val = int(parts[0])
                                if "GB" in vram.upper():
                                    vram_mb = vram_val * 1024
                                else:
                                    vram_mb = vram_val
                            except ValueError:
                                pass

                    gpu_info = {
                        "vendor": "Apple",
                        "name": gpu_name,
                        "vram_mb": vram_mb,
                        "compute_capability": None,
                        "backend": "CoreMLExecutionProvider",
                        "metal_supported": True
                    }
                    return gpu_info
    except (FileNotFoundError, subprocess.TimeoutExpired, json.JSONDecodeError, Exception):
        pass

    # Method 2: PyTorch MPS
    try:
        import torch
        if hasattr(torch.backends, 'mps') and torch.backends.mps.is_available():
            # Get chip name from sysctl
            chip_result = subprocess.run(
                ["sysctl", "-n", "machdep.cpu.brand_string"],
                capture_output=True,
                text=True,
                timeout=5
            )
            chip_name = "Apple Silicon"
            if chip_result.returncode == 0:
                chip_name = chip_result.stdout.strip()

            gpu_info = {
                "vendor": "Apple",
                "name": chip_name,
                "vram_mb": 0,  # Unified memory - hard to determine GPU portion
                "compute_capability": None,
                "backend": "CoreMLExecutionProvider",
                "metal_supported": True
            }
            return gpu_info
    except ImportError:
        pass
    except Exception:
        pass

    return None


def detect_intel_gpu() -> Optional[Dict[str, Any]]:
    """Detect Intel integrated/discrete GPU"""
    gpu_info = None

    # Check lspci on Linux
    if platform.system() == "Linux":
        try:
            result = subprocess.run(
                ["lspci", "-nn"],
                capture_output=True,
                text=True,
                timeout=5
            )
            if result.returncode == 0:
                for line in result.stdout.split("\n"):
                    if "VGA" in line or "3D" in line:
                        # Check for Intel vendor ID [8086:]
                        if "[8086:" in line:
                            parts = line.split(": ", 1)
                            gpu_name = parts[1] if len(parts) > 1 else "Intel GPU"
                            if "[" in gpu_name:
                                gpu_name = gpu_name.split("[")[0].strip()

                            gpu_info = {
                                "vendor": "Intel",
                                "name": gpu_name,
                                "vram_mb": 0,
                                "compute_capability": None,
                                "backend": "CPUExecutionProvider",  # Intel GPUs not well supported in ONNX Runtime
                                "note": "Intel GPUs have limited ONNX Runtime support"
                            }
                            return gpu_info
        except (FileNotFoundError, subprocess.TimeoutExpired, Exception):
            pass

    return None


def check_rocm_installed() -> bool:
    """Check if ROCm is installed on the system"""
    # Check for rocm-smi
    try:
        result = subprocess.run(
            ["rocm-smi", "--version"],
            capture_output=True,
            timeout=5
        )
        return result.returncode == 0
    except (FileNotFoundError, subprocess.TimeoutExpired):
        pass

    # Check for ROCm directory
    rocm_paths = [
        "/opt/rocm",
        "/opt/rocm-*"
    ]
    for path in rocm_paths:
        if os.path.exists(path):
            return True

    return False


def check_cuda_toolkit_installed() -> bool:
    """Check if CUDA toolkit is installed"""
    try:
        result = subprocess.run(
            ["nvcc", "--version"],
            capture_output=True,
            timeout=5
        )
        return result.returncode == 0
    except (FileNotFoundError, subprocess.TimeoutExpired):
        pass

    # Check CUDA directory
    cuda_paths = [
        "/usr/local/cuda",
        "C:\\Program Files\\NVIDIA GPU Computing Toolkit\\CUDA"
    ]
    for path in cuda_paths:
        if os.path.exists(path):
            return True

    return False


def get_onnxruntime_providers() -> List[str]:
    """Get available ONNX Runtime execution providers"""
    try:
        import onnxruntime as ort
        return ort.get_available_providers()
    except ImportError:
        return []
    except Exception:
        return []


def detect_all_gpus() -> Dict[str, Any]:
    """
    Detect all available GPUs and recommend the best STT backend.

    Returns a JSON-serializable dictionary with:
    - gpus: List of detected GPUs
    - recommended_backend: Best execution provider for STT
    - system_info: OS and platform details
    - onnxruntime_providers: Available ONNX Runtime providers
    """
    result = {
        "success": True,
        "gpus": [],
        "recommended_backend": "CPUExecutionProvider",
        "recommended_gpu": None,
        "system_info": {
            "os": platform.system(),
            "os_version": platform.version(),
            "architecture": platform.machine(),
            "python_version": platform.python_version()
        },
        "onnxruntime_providers": get_onnxruntime_providers(),
        "cuda_toolkit_installed": check_cuda_toolkit_installed(),
        "rocm_installed": check_rocm_installed()
    }

    # Detect GPUs in priority order
    nvidia_gpu = detect_nvidia_gpu()
    if nvidia_gpu:
        result["gpus"].append(nvidia_gpu)
        result["recommended_backend"] = "CUDAExecutionProvider"
        result["recommended_gpu"] = nvidia_gpu

    amd_gpu = detect_amd_gpu()
    if amd_gpu:
        result["gpus"].append(amd_gpu)
        # Only recommend AMD if no NVIDIA found
        if not nvidia_gpu:
            # Check if ROCm is actually installed
            if result["rocm_installed"] or amd_gpu.get("rocm_installed"):
                result["recommended_backend"] = "MIGraphXExecutionProvider"
            else:
                # AMD GPU exists but ROCm not installed - still recommend AMD backend
                # Installation will handle ROCm setup
                result["recommended_backend"] = "MIGraphXExecutionProvider"
            result["recommended_gpu"] = amd_gpu

    apple_gpu = detect_apple_gpu()
    if apple_gpu:
        result["gpus"].append(apple_gpu)
        # Apple Silicon is high priority on macOS
        if not nvidia_gpu:
            result["recommended_backend"] = "CoreMLExecutionProvider"
            result["recommended_gpu"] = apple_gpu

    intel_gpu = detect_intel_gpu()
    if intel_gpu:
        result["gpus"].append(intel_gpu)
        # Intel is lowest priority, only use if nothing else available
        if not result["gpus"]:
            result["recommended_backend"] = "CPUExecutionProvider"
            result["recommended_gpu"] = intel_gpu

    # If no GPU detected, CPU is the fallback
    if not result["gpus"]:
        result["gpus"].append({
            "vendor": "CPU",
            "name": platform.processor() or "Unknown CPU",
            "backend": "CPUExecutionProvider"
        })
        result["recommended_gpu"] = result["gpus"][0]

    return result


def get_installation_commands(backend: str, system: str) -> Dict[str, Any]:
    """
    Get the pip/conda installation commands for the recommended backend.

    Returns commands that the Rust backend can execute for automatic installation.
    """
    commands = {
        "backend": backend,
        "system": system,
        "pip_packages": [],
        "system_packages": [],
        "post_install_check": None,
        "notes": []
    }

    if backend == "CUDAExecutionProvider":
        commands["pip_packages"] = [
            "onnxruntime-gpu",
            "sherpa-onnx"
        ]
        commands["notes"] = [
            "Requires NVIDIA GPU with CUDA support",
            "CUDA toolkit will be installed via pip dependencies"
        ]
        commands["post_install_check"] = "import onnxruntime; assert 'CUDAExecutionProvider' in onnxruntime.get_available_providers()"

    elif backend == "MIGraphXExecutionProvider":
        if system == "Linux":
            commands["pip_packages"] = [
                "onnxruntime-rocm",  # AMD ROCm support
                "sherpa-onnx"
            ]
            commands["system_packages"] = [
                "rocm-dev"  # May need to be installed separately
            ]
            commands["notes"] = [
                "Requires AMD GPU with ROCm support",
                "ROCm 5.x+ recommended",
                "May need to add user to 'video' and 'render' groups"
            ]
        else:
            # Windows/macOS - fallback to CPU for AMD
            commands["pip_packages"] = [
                "onnxruntime",
                "sherpa-onnx"
            ]
            commands["notes"] = [
                "AMD ROCm only supported on Linux",
                "Falling back to CPU execution"
            ]
            commands["backend"] = "CPUExecutionProvider"

    elif backend == "CoreMLExecutionProvider":
        commands["pip_packages"] = [
            "onnxruntime-silicon",  # Apple Silicon optimized
            "sherpa-onnx"
        ]
        commands["notes"] = [
            "Optimized for Apple Silicon (M1/M2/M3/M4)",
            "Uses Metal for GPU acceleration"
        ]

    else:  # CPUExecutionProvider
        commands["pip_packages"] = [
            "onnxruntime",
            "sherpa-onnx"
        ]
        commands["notes"] = [
            "CPU-only execution",
            "Works on all platforms but slower than GPU"
        ]

    # Common packages for all backends
    commands["pip_packages"].extend([
        "numpy",
        "soundfile"
    ])

    return commands


def main():
    """Main entry point - detect GPUs and output JSON"""
    try:
        if len(sys.argv) > 1:
            command = sys.argv[1]

            if command == "detect":
                result = detect_all_gpus()
                print(json.dumps(result, indent=2))

            elif command == "install-commands":
                # Get installation commands for a specific backend
                detection = detect_all_gpus()
                backend = sys.argv[2] if len(sys.argv) > 2 else detection["recommended_backend"]
                commands = get_installation_commands(backend, detection["system_info"]["os"])
                print(json.dumps(commands, indent=2))

            elif command == "providers":
                # Just list available ONNX Runtime providers
                providers = get_onnxruntime_providers()
                print(json.dumps({"providers": providers}))

            else:
                print(json.dumps({
                    "error": f"Unknown command: {command}",
                    "usage": "gpu_detector.py [detect|install-commands|providers]"
                }))
                sys.exit(1)
        else:
            # Default: full detection
            result = detect_all_gpus()
            print(json.dumps(result, indent=2))

    except Exception as e:
        print(json.dumps({
            "success": False,
            "error": str(e),
            "type": "detection_error"
        }))
        sys.exit(1)


if __name__ == "__main__":
    main()
