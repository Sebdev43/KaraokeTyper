# Task: Gestion GPU Vulkan pour whisper-rs

## Résumé Exécutif

L'analyse révèle que whisper-rs supporte Vulkan mais avec des limitations importantes:
- **API `gpu_device(i32)` existe** mais n'est pas utilisée actuellement
- **Pas d'API native** pour énumérer les GPU ou interroger la VRAM
- **Fallback automatique implémenté** mais pas de notification utilisateur
- **Crashes fréquents** sur certaines configurations (AMD Windows, Flash Attention)

---

## Codebase Context

### Fichiers GPU Existants

| Fichier | Lignes | Rôle |
|---------|--------|------|
| `src-tauri/src/gpu.rs` | 1-148 | Détection GPU (informative uniquement) |
| `src-tauri/src/stt_whisper.rs` | 90-182 | Initialisation Whisper avec GPU |
| `src-tauri/src/config.rs` | 6-20 | Stockage préférence `use_gpu: bool` |
| `src-tauri/src/lib.rs` | 67-73, 947-957 | Commande `get_gpu_info()` |

### Implémentation Actuelle

**GPU Detection** (`gpu.rs:15-18`):
```rust
fn is_gpu_enabled() -> bool {
    cfg!(feature = "gpu-vulkan") || cfg!(feature = "gpu-metal")
}
```

**GPU Initialization avec Fallback** (`stt_whisper.rs:138-166`):
```rust
let ctx = if should_use_gpu {
    match WhisperContext::new_with_params(model_path_str, gpu_params) {
        Ok(ctx) => ctx,
        Err(gpu_error) => {
            // Fallback automatique vers CPU
            WhisperContext::new_with_params(model_path_str, cpu_params)?
        }
    }
}
```

### Limitations Identifiées

1. **Pas de sélection de device** - `gpu_device()` non utilisé
2. **Pas d'énumération GPU** - Impossible de lister les GPU disponibles
3. **Fallback silencieux** - L'utilisateur ne sait pas si GPU a échoué
4. **Pas de monitoring VRAM** - Aucune visibilité sur l'utilisation mémoire
5. **Pas de détection de crash** - Pas de compteur d'échecs consécutifs

---

## Documentation Insights

### API WhisperContextParameters

```rust
use whisper_rs::WhisperContextParameters;

let params = WhisperContextParameters::new()
    .use_gpu(true)           // Activer GPU
    .gpu_device(0)           // Sélectionner GPU par index (0-based)
    .flash_attn(false);      // Flash attention (incompatible avec DTW)
```

| Méthode | Signature | Description |
|---------|-----------|-------------|
| `use_gpu()` | `fn use_gpu(&mut self, bool) -> &mut Self` | Active/désactive GPU |
| `gpu_device()` | `fn gpu_device(&mut self, c_int) -> &mut Self` | **Index du GPU (0, 1, 2...)** |
| `flash_attn()` | `fn flash_attn(&mut self, bool) -> &mut Self` | Flash attention (boost perf) |

### Variable d'Environnement

```bash
# Sélection GPU via env var (ggml backend)
export GGML_VK_VISIBLE_DEVICES=0      # Premier GPU
export GGML_VK_VISIBLE_DEVICES=1      # Deuxième GPU
export GGML_VK_VISIBLE_DEVICES=0,1    # Multi-GPU
```

### Erreurs GPU

```rust
pub enum WhisperError {
    InitError,          // Échec init GPU (le plus courant)
    GenericError(i32),  // Codes d'erreur Vulkan
    // ...
}
```

**Logs stderr** pour diagnostic:
```
ggml_vulkan: Found 1 Vulkan devices:
ggml_vulkan: 0 = AMD Radeon RX 6600 XT (RADV NAVI23)
```

---

## Research Findings

### VRAM Requirements par Modèle

| Modèle | Paramètres | Taille Fichier | VRAM Requise |
|--------|------------|----------------|--------------|
| **tiny** | 39M | 75 MB | ~1 GB |
| **base** | 74M | 142 MB | ~1 GB |
| **small** | 244M | 466 MB | **~2 GB** |
| **medium** | 769M | 1.5 GB | ~5 GB |
| **large-v3** | 1550M | 2.9 GB | ~10 GB |

**Note**: whisper.cpp nécessite ~2x la taille du modèle en VRAM.

### Crashes Vulkan Courants

1. **ErrorOutOfDeviceMemory**
   - Cause: VRAM insuffisante
   - Solution: Utiliser modèle plus petit ou CPU

2. **ErrorDeviceLost**
   - Cause: Flash Attention, timeout driver, ressources épuisées
   - Solution: Désactiver Flash Attention, mettre à jour drivers

3. **Stack Overflow (NVIDIA)**
   - Cause: Bug dans cooperative matrix shaders
   - Affecte: RTX 4090/4060 Ti
   - Solution: Désactiver Vulkan ou attendre fix driver

### Stabilité par Plateforme

| Plateforme | Stabilité | Notes |
|------------|-----------|-------|
| **Linux** | Bonne | Meilleur support Vulkan |
| **Windows** | Variable | Bugs fréquents, dépend du driver |
| **macOS** | N/A | Utiliser Metal, pas Vulkan |

### AMD vs NVIDIA

- **AMD Linux**: Fonctionne bien avec RADV (Mesa)
- **AMD Windows**: Problèmes fréquents, exits silencieux
- **NVIDIA**: Stable sauf bugs spécifiques (RTX 40xx)

---

## Key Files

- `src-tauri/src/gpu.rs:20-76` - Détection Linux via `vulkaninfo`
- `src-tauri/src/stt_whisper.rs:127-132` - Two-gate GPU check
- `src-tauri/src/stt_whisper.rs:138-166` - GPU init avec fallback
- `src-tauri/src/config.rs:18-19` - Champ `use_gpu: bool`
- `src-tauri/Cargo.toml:84-86` - Feature flags `gpu-vulkan`, `gpu-metal`
- `src/components/SettingsWindow.tsx:732-787` - UI GPU toggle

---

## Patterns to Follow

### Pattern 1: Sélection de Device GPU

```rust
// Utiliser gpu_device() pour multi-GPU
let mut params = WhisperContextParameters::default();
params.use_gpu(true);
params.gpu_device(device_index as i32);  // 0, 1, 2...
```

### Pattern 2: Fallback avec Notification

```rust
match WhisperContext::new_with_params(path, gpu_params) {
    Ok(ctx) => {
        // Notifier frontend: GPU actif
        app_handle.emit("gpu-status", json!({"mode": "gpu", "device": device_name}));
        ctx
    }
    Err(e) => {
        // Notifier frontend: fallback CPU
        app_handle.emit("gpu-status", json!({"mode": "cpu", "reason": format!("{:?}", e)}));
        WhisperContext::new_with_params(path, cpu_params)?
    }
}
```

### Pattern 3: Compteur d'Échecs

```rust
static GPU_FAILURES: AtomicU32 = AtomicU32::new(0);
const MAX_GPU_FAILURES: u32 = 3;

fn should_try_gpu() -> bool {
    GPU_FAILURES.load(Ordering::Relaxed) < MAX_GPU_FAILURES
}

fn record_gpu_failure() {
    let failures = GPU_FAILURES.fetch_add(1, Ordering::Relaxed) + 1;
    if failures >= MAX_GPU_FAILURES {
        eprintln!("[GPU] Désactivation auto après {} échecs consécutifs", failures);
    }
}

fn reset_gpu_failures() {
    GPU_FAILURES.store(0, Ordering::Relaxed);
}
```

### Pattern 4: Validation VRAM

```rust
fn estimate_vram_mb(model_name: &str) -> u32 {
    match model_name {
        "ggml-tiny" | "ggml-tiny.en" => 1024,
        "ggml-base" | "ggml-base.en" => 1024,
        "ggml-small" | "ggml-small.en" => 2048,
        "ggml-medium" | "ggml-medium.en" => 5120,
        "ggml-large" | "ggml-large-v3" => 10240,
        _ => 2048, // Défaut conservateur
    }
}
```

---

## Dependencies

### Crates Rust

- `whisper-rs = "0.14"` (ou plus récent)
- Feature: `whisper-rs/vulkan` ou `whisper-rs/metal`

### Prérequis Système

- **Linux**: `vulkaninfo` (optionnel, pour détection)
- **Windows**: Vulkan SDK (pour compilation uniquement)
- **macOS**: Xcode avec Metal support

### Variables d'Environnement

- `GGML_VK_VISIBLE_DEVICES` - Sélection GPU Vulkan
- `GGML_VK_DEVICE0_MEMORY` - Limite mémoire manuelle (MB)

---

## Améliorations Recommandées

### Priorité Haute

1. **Ajouter `gpu_device()` à l'initialisation** - Permettre sélection multi-GPU
2. **Événements Tauri pour statut GPU** - Notifier frontend du mode actif
3. **Compteur d'échecs GPU** - Désactiver auto après N crashes
4. **Afficher raison du fallback** - UI indique pourquoi CPU est utilisé

### Priorité Moyenne

5. **Liste des GPU disponibles** - Commande `list_gpu_devices()`
6. **Estimation VRAM** - Avertir si modèle trop gros pour GPU
7. **Option Flash Attention** - Toggle dans Settings (avec warning)
8. **Mode "auto"** - Essayer GPU, fallback silencieux si échec

### Priorité Basse

9. **Monitoring VRAM temps réel** - Afficher utilisation mémoire
10. **Blacklist drivers problématiques** - Désactiver GPU pour drivers bugués
11. **Timeout transcription** - Tuer et relancer si GPU bloqué

---

## Risques et Mitigations

| Risque | Impact | Mitigation |
|--------|--------|------------|
| Crash GPU = crash app | Élevé | Fallback CPU implémenté |
| AMD Windows instable | Moyen | Défaut CPU, GPU opt-in |
| Flash Attention crash | Moyen | Désactivé par défaut |
| Multi-GPU non testé | Faible | Utiliser device 0 par défaut |

---

## Prochaine Étape

Exécuter `/apex:2-plan 02-gpu-vulkan-management` pour créer le plan d'implémentation.
