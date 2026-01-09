# Implementation Report: GPU Acceleration Settings

## Summary

Successfully implemented GPU acceleration status display in Settings with optional GPU features.

## Changes Made

### Backend (Rust)

#### `src-tauri/Cargo.toml`
- Added `whisper-rs = "0.14"` as a cross-platform dependency (CPU by default)
- Added optional Cargo features for GPU acceleration:
  - `gpu-vulkan` - Enables Vulkan backend for Linux/Windows (NVIDIA, AMD, Intel)
  - `gpu-metal` - Enables Metal backend for macOS (Apple Silicon)
- GPU features are opt-in to avoid build failures without SDK

#### `src-tauri/src/gpu.rs` (NEW)
- Created GPU detection module with platform-specific implementations
- Detects GPU hardware at runtime (vulkaninfo, lspci, system_profiler, wmic)
- Checks compile-time features to determine if GPU is actually used
- Returns `GpuDetectionResult` with `available`, `backend`, and `device_name`

#### `src-tauri/src/lib.rs`
- Added `mod gpu;` import
- Added `GpuInfo` struct with Serialize and specta::Type derives
- Added `get_gpu_info()` Tauri command
- Registered command in `collect_commands!` macro

### Frontend (React/TypeScript)

#### `src/stores/settingsStore.ts`
- Added `GpuInfo` import from bindings
- Added `gpuInfo: GpuInfo | null` state field
- Added `setGpuInfo` action

#### `src/components/SettingsWindow.tsx`
- Added `Cpu` icon import from lucide-react
- Added `gpuInfo` and `setGpuInfo` from settings store
- Added `loadGpuInfo` callback to fetch GPU status on mount
- Added "Performance" Card with:
  - GPU Acceleration label
  - Device name display
  - Status badge (Active/CPU Only)
  - Backend description text

## Build Instructions

### CPU Mode (Default)
```bash
cd src-tauri
cargo build
# or
npm run tauri dev
```

### GPU Mode

**Linux/Windows (Vulkan):**
Requires: `libvulkan-dev`, `glslang-tools` or `shaderc`
```bash
cargo build --features gpu-vulkan
```

**macOS (Metal):**
Requires: Xcode Command Line Tools (built-in)
```bash
cargo build --features gpu-metal
```

## UI Preview

The Settings window now shows a "Performance" card after "Speech Recognition":

```
┌─────────────────────────────────────────┐
│ 🔧 Performance                          │
├─────────────────────────────────────────┤
│ GPU Acceleration                        │
│ [GPU Name or "Detecting GPU..."]        │
│                        [CPU Only] badge │
│                                         │
│ No compatible GPU detected. Using CPU   │
│ for transcription.                      │
└─────────────────────────────────────────┘
```

When GPU is available (compiled with features):
```
┌─────────────────────────────────────────┐
│ 🔧 Performance                          │
├─────────────────────────────────────────┤
│ GPU Acceleration                        │
│ NVIDIA GeForce RTX 3080                 │
│                      [Vulkan Active]    │
│                                         │
│ Using Vulkan backend for faster         │
│ transcription                           │
└─────────────────────────────────────────┘
```

## Testing Checklist

- [x] `cargo build` succeeds in CPU mode
- [x] TypeScript bindings generated with `GpuInfo` type
- [x] `npm run tauri dev` starts successfully
- [x] Performance card visible in Settings
- [ ] GPU status displays correctly (manual verification)
- [ ] GPU acceleration works when compiled with features (requires SDK)

## CI/CD Notes

To enable GPU builds in CI, install:

**Linux (GitHub Actions):**
```yaml
- name: Install Vulkan SDK
  run: |
    sudo apt-get update
    sudo apt-get install -y libvulkan-dev glslang-tools
```

**macOS (GitHub Actions):**
Metal is available natively, no extra setup needed.

**Windows (GitHub Actions):**
Install LunarG Vulkan SDK or use Vulkan drivers.

## Design Decisions

1. **GPU features opt-in**: Made GPU features optional Cargo features instead of default to:
   - Allow development without SDK installation
   - Prevent CI build failures
   - Give users control over build configuration

2. **Runtime detection**: Even with GPU features disabled, the module detects GPU hardware for informational purposes, but reports "CPU Only" as the active backend.

3. **No user toggle**: GPU acceleration is automatic when compiled with features. No runtime toggle because whisper-rs doesn't support runtime switching.

## Files Changed

| File | Action |
|------|--------|
| `src-tauri/Cargo.toml` | Modified - GPU features |
| `src-tauri/src/gpu.rs` | Created - GPU detection |
| `src-tauri/src/lib.rs` | Modified - GpuInfo struct + command |
| `src/stores/settingsStore.ts` | Modified - GPU state |
| `src/components/SettingsWindow.tsx` | Modified - Performance card |
| `src/bindings.ts` | Auto-generated |
