# Task: GPU Acceleration Settings - Cross-Platform

## Executive Summary

**Objectif:** Permettre à l'utilisateur d'activer l'accélération GPU dans les Settings pour une transcription Whisper plus rapide.

**Contrainte majeure découverte:** whisper-rs ne supporte PAS le switching runtime CPU/GPU. Le backend GPU est déterminé à la **compilation** via Cargo features.

**Solution recommandée:** Compiler avec Vulkan (cross-platform), détecter le GPU au runtime, fallback automatique sur CPU si pas de GPU.

---

## Codebase Context

### Current Whisper Implementation

**File: `src-tauri/src/stt_whisper.rs:118-132`**
```rust
#[cfg(not(debug_assertions))]
let params = WhisperContextParameters::default();

let ctx = WhisperContext::new_with_params(
    self.model_path.to_str().unwrap_or_default(),
    params,
)
```
- Utilise `WhisperContextParameters::default()` - aucune config GPU
- GPU serait automatique si feature Cargo activée

**File: `src-tauri/Cargo.toml:66`**
```toml
whisper-rs = "0.14"
```
- Pas de features GPU activées actuellement

### Settings System

**File: `src-tauri/src/config.rs:6-17`**
```rust
pub struct AppConfig {
    pub shortcut: String,
    pub audio_device_index: u32,
    pub audio_device_name: Option<String>,
    pub model: String,
    pub key_delay_ms: u32,
    pub language: String,
    pub model_path: Option<String>,
    // TODO: Add gpu_enabled: bool
}
```

**File: `src/components/SettingsWindow.tsx:822-843`**
- Pattern existant pour toggle Switch (autostart)
- Utilise shadcn/ui Switch + Badge

### Platform-Specific Pattern

**File: `src-tauri/src/autostart.rs:4-17`**
```rust
#[cfg(target_os = "linux")]
pub fn get_autostart_path() -> Option<PathBuf> { ... }

#[cfg(target_os = "macos")]
pub fn get_autostart_path() -> Option<PathBuf> { ... }
```

---

## Documentation Insights

### whisper-rs GPU Features

| Feature | Platform | Hardware |
|---------|----------|----------|
| `cuda` | Linux, Windows | NVIDIA GPU |
| `metal` | macOS | Apple Silicon |
| `vulkan` | All | NVIDIA, AMD, Intel |
| `hipblas` | Linux only | AMD ROCm |

### Key Limitation

**GPU backend is compile-time only:**
- Pas d'API `is_gpu_available()`
- Pas de `set_gpu_device()`
- Le feature flag active automatiquement le GPU au runtime

### WhisperContextParameters

Pas de paramètres GPU exposés - tout est automatique si feature activée.

---

## Research Findings

### Best Cross-Platform Strategy

1. **Vulkan** = meilleur choix cross-platform
   - Fonctionne sur Linux, Windows, macOS (via MoltenVK)
   - Supporte NVIDIA, AMD, Intel
   - Pas d'installation externe requise (juste drivers)

2. **CUDA** = meilleure performance NVIDIA
   - Requiert CUDA Toolkit installé
   - Linux et Windows uniquement

3. **Metal** = optimal pour macOS
   - Natif sur Apple Silicon
   - Pas de dépendance externe

### AMD Support Issues

- Support GPU AMD problématique avec whisper.cpp
- Vulkan détecte le GPU mais fallback souvent sur CPU
- **Recommandation:** Ne pas prioriser AMD, focus NVIDIA + Intel

### Runtime Detection

Pour détecter la présence d'un GPU sans whisper-rs:
- Utiliser `wgpu` crate pour énumérer les GPUs
- Variables d'environnement: `WGPU_ADAPTER_NAME`, `WGPU_BACKEND`

---

## Key Files

| File | Line | Purpose |
|------|------|---------|
| `src-tauri/src/stt_whisper.rs` | 118-132 | WhisperContext initialization |
| `src-tauri/src/config.rs` | 6-17 | AppConfig struct (add gpu_enabled) |
| `src-tauri/Cargo.toml` | 66 | whisper-rs dependency |
| `src/components/SettingsWindow.tsx` | 822-843 | Toggle pattern (autostart) |
| `src-tauri/src/lib.rs` | 71-116 | SttEngine wrapper |

---

## Patterns to Follow

### 1. Settings Toggle (shadcn/ui)
```tsx
<Switch
  checked={gpuEnabled}
  onCheckedChange={handleToggleGpu}
  disabled={!gpuAvailable}
/>
```

### 2. Platform-Specific Cargo Features
```toml
[target.'cfg(target_os = "macos")'.dependencies]
whisper-rs = { version = "0.14", features = ["metal"] }

[target.'cfg(target_os = "linux")'.dependencies]
whisper-rs = { version = "0.14", features = ["vulkan"] }

[target.'cfg(target_os = "windows")'.dependencies]
whisper-rs = { version = "0.14", features = ["vulkan"] }
```

### 3. Runtime GPU Detection (wgpu)
```rust
fn detect_gpu() -> bool {
    let instance = wgpu::Instance::default();
    let adapters: Vec<_> = instance.enumerate_adapters(wgpu::Backends::all()).collect();
    !adapters.is_empty()
}
```

---

## Dependencies

### New Crates Required
```toml
wgpu = "0.19"  # GPU detection (optional, for UI feedback)
```

### System Requirements
- **Linux:** Vulkan drivers (mesa-vulkan-drivers)
- **Windows:** GPU drivers avec Vulkan support
- **macOS:** Aucune (Metal natif)

---

## Implementation Options

### Option A: Compile-Time GPU (Recommandé)

**Avantages:**
- Simple à implémenter
- GPU automatique si disponible
- Fallback CPU transparent

**Inconvénients:**
- Pas de toggle runtime dans Settings
- L'utilisateur ne peut pas désactiver le GPU

**Implementation:**
1. Ajouter Vulkan feature à Cargo.toml
2. Compiler avec GPU activé
3. whisper-rs utilise GPU automatiquement si disponible

### Option B: Multiple Binaires

**Avantages:**
- Toggle runtime possible
- CPU-only pour machines sans GPU

**Inconvénients:**
- Distribution complexe (2 binaires)
- Maintenance double

### Option C: Variable d'Environnement

**Avantages:**
- Toggle sans recompilation
- Un seul binaire

**Implementation:**
1. Compiler avec Vulkan
2. `GGML_VULKAN_DISABLE=1` pour forcer CPU
3. Settings modifie la variable avant init

---

## Recommended Approach

### Phase 1: Enable GPU by Default
1. Modifier `Cargo.toml` pour activer Vulkan cross-platform
2. Ajouter wgpu pour détection GPU (UI feedback)
3. Afficher status GPU dans Settings (read-only)

### Phase 2: User Control (Optional)
1. Ajouter `gpu_enabled` à AppConfig
2. Utiliser variable d'environnement `GGML_VULKAN_DISABLE`
3. Redémarrer STT engine si toggle change

---

## Risks & Blockers

1. **Build Time:** Vulkan augmente le temps de compilation
2. **Binary Size:** +5-10 MB avec GPU support
3. **AMD Issues:** Support instable sur certaines cartes
4. **macOS Intel:** Metal non disponible, Vulkan via MoltenVK

---

## Next Step

Run `/apex:2-plan` pour créer le plan d'implémentation détaillé.
