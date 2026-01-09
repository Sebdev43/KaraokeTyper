# Implementation Plan: GPU Acceleration Settings

## Overview

**Approche:** Activer le GPU automatiquement avec backend optimal par plateforme (Metal/Vulkan), afficher le status GPU dans Settings en lecture seule.

**Stratégie GPU:**
- **macOS:** Metal (natif Apple Silicon)
- **Linux:** Vulkan (cross-vendor)
- **Windows:** Vulkan (cross-vendor)

**Pas de toggle utilisateur** - GPU utilisé automatiquement si disponible, fallback transparent sur CPU.

---

## Dependencies

**Ordre d'exécution:**
1. `Cargo.toml` - Ajouter features GPU (doit être fait en premier)
2. `gpu.rs` - Nouveau module détection GPU
3. `lib.rs` - Exposer commande Tauri pour status GPU
4. `bindings.ts` - Auto-généré après compilation
5. `settingsStore.ts` - État GPU
6. `SettingsWindow.tsx` - UI status GPU

---

## File Changes

### `src-tauri/Cargo.toml`

- **Action:** Ajouter dépendances GPU platform-specific pour whisper-rs
- **Localisation:** Après la ligne 66 (whisper-rs actuel)
- **Modification:** Remplacer `whisper-rs = "0.14"` par configuration platform-specific
- **Ajouter:** Section `[target.'cfg(...)'.dependencies]` pour chaque plateforme
- **macOS:** Feature `metal` pour Apple Silicon
- **Linux:** Feature `vulkan` pour support cross-vendor
- **Windows:** Feature `vulkan` pour support cross-vendor
- **Consider:** Ne pas ajouter wgpu pour l'instant (trop lourd), utiliser détection simple

### `src-tauri/src/gpu.rs` (NOUVEAU FICHIER)

- **Action:** Créer module de détection GPU
- **Fonction `detect_gpu_info()`:** Retourne struct GpuInfo avec:
  - `available: bool` - GPU détecté ou non
  - `backend: String` - "Metal", "Vulkan", "CPU"
  - `device_name: Option<String>` - Nom du GPU si disponible
- **Pattern:** Utiliser `#[cfg(target_os = "...")]` comme dans `autostart.rs`
- **Linux:** Lire `/proc/driver/nvidia/version` ou vérifier Vulkan via commande `vulkaninfo`
- **macOS:** Utiliser `system_profiler SPDisplaysDataType` pour nom GPU
- **Windows:** Utiliser `wmic path win32_VideoController get name`
- **Fallback:** Si détection échoue, retourner backend compilé comme "available"

### `src-tauri/src/lib.rs`

- **Action 1:** Ajouter import du module gpu
- **Localisation:** Ligne ~10, après autres imports de modules
- **Pattern:** Suivre `mod autostart;`

- **Action 2:** Ajouter struct GpuInfo pour Tauri/specta
- **Localisation:** Section types (après ligne 65)
- **Dériver:** `Clone, Serialize, Type` comme autres structs

- **Action 3:** Ajouter commande Tauri `get_gpu_info`
- **Localisation:** Après autres commandes get (vers ligne 680)
- **Pattern:** Similaire à `get_system_info()`
- **Retour:** `Result<GpuInfo, String>`

- **Action 4:** Enregistrer commande dans `generate_handler!`
- **Localisation:** Macro generate_handler (vers ligne 1495)
- **Ajouter:** `get_gpu_info` à la liste

### `src/stores/settingsStore.ts`

- **Action:** Ajouter état GPU au store Zustand
- **Ajouter champ:** `gpuInfo: GpuInfo | null`
- **Ajouter action:** `setGpuInfo: (info: GpuInfo) => void`
- **Initial value:** `null`

### `src/components/SettingsWindow.tsx`

- **Action 1:** Importer GpuInfo depuis bindings
- **Localisation:** Imports en haut du fichier

- **Action 2:** Ajouter état local pour GPU info
- **Utiliser:** `gpuInfo` depuis settingsStore

- **Action 3:** Charger GPU info au mount
- **Dans:** `useEffect` existant ou nouveau
- **Appeler:** `commands.getGpuInfo()`
- **Stocker:** Résultat dans store

- **Action 4:** Ajouter Card "Performance" dans UI
- **Localisation:** Après la Card "Speech Recognition" (vers ligne 710)
- **Pattern:** Suivre structure des autres Cards avec CardHeader/CardTitle/CardContent
- **Icône:** Utiliser `Cpu` ou `Zap` de lucide-react
- **Contenu:**
  - Label "GPU Acceleration"
  - Badge status: "Active" (vert) si available, "CPU Only" (gris) sinon
  - Texte descriptif: Nom du GPU ou "No GPU detected"
  - Texte backend: "Using Metal/Vulkan"
- **Consider:** Pas de Switch, lecture seule uniquement

### `src/bindings.ts` (AUTO-GÉNÉRÉ)

- **Action:** Aucune modification manuelle
- **Note:** Sera régénéré automatiquement par tauri-specta après compilation Rust
- **Vérifier:** Type GpuInfo présent après `cargo build`

---

## Testing Strategy

### Compilation Tests

- **Test 1:** `cargo build` sur Linux - Vérifier compilation Vulkan
- **Test 2:** `cargo build` sur macOS - Vérifier compilation Metal (si disponible)
- **Test 3:** `cargo build` sur Windows - Vérifier compilation Vulkan (CI)

### Runtime Tests

- **Test 1:** Lancer app sur machine avec GPU NVIDIA
  - Vérifier: Settings affiche "GPU Active" + nom GPU
  - Vérifier: Logs montrent utilisation Vulkan

- **Test 2:** Lancer app sur machine sans GPU dédié
  - Vérifier: Settings affiche "CPU Only"
  - Vérifier: App fonctionne normalement

- **Test 3:** Transcription performance
  - Comparer temps transcription GPU vs CPU (si possible)
  - Vérifier: Pas de régression si GPU indisponible

### Manual Verification

1. Ouvrir Settings
2. Vérifier Card "Performance" visible
3. Vérifier Badge status correct
4. Vérifier nom GPU affiché (si disponible)
5. Faire transcription, vérifier fonctionnement

---

## Documentation

- **Pas de mise à jour README** pour cette feature (interne)
- **Mettre à jour CLAUDE.md** si patterns GPU ajoutés

---

## Rollout Considerations

### Build Time Impact

- Vulkan: +30-60s compilation initiale
- Metal: +20-30s compilation initiale
- Builds suivants: Impact minimal (cache)

### Binary Size Impact

- Estimation: +5-15 MB selon plateforme
- Acceptable pour amélioration performance

### Fallback Behavior

- Si GPU init échoue: whisper-rs fallback automatique sur CPU
- Pas de crash, pas d'action utilisateur requise
- Logs indiqueront le fallback

### CI/CD Impact

- GitHub Actions: Vulkan SDK peut être nécessaire pour Linux builds
- macOS runners: Metal disponible nativement
- Windows runners: Vulkan SDK à installer

---

## Risk Mitigation

| Risque | Mitigation |
|--------|------------|
| Build échoue avec Vulkan | Garder branche sans GPU comme fallback |
| Performance dégradée sur AMD | Documenter dans release notes |
| Metal non disponible macOS Intel | Vulkan fallback automatique |
| CI builds cassés | Ajouter Vulkan SDK dans workflow |

---

## Next Step

Run `/apex:3-execute 01-gpu-acceleration-settings` pour implémenter le plan.
