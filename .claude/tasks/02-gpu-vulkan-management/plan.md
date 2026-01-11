# Implementation Plan: Gestion GPU Vulkan Robuste

## Overview

Améliorer la gestion GPU pour offrir une expérience fiable et transparente:
- Notifier l'utilisateur du mode actif (GPU/CPU)
- Compter les échecs GPU et désactiver automatiquement après 3 crashes
- Afficher la raison du fallback dans l'UI
- Préparer le support multi-GPU (device selection)

## Dependencies

**Ordre d'implémentation obligatoire:**
1. `config.rs` - Nouveaux champs config
2. `stt_whisper.rs` - Logique GPU améliorée
3. `lib.rs` - Commandes et événements Tauri
4. `bindings.ts` - Types TypeScript (régénéré)
5. `settingsStore.ts` - État frontend
6. `SettingsWindow.tsx` - Interface utilisateur

---

## File Changes

### `src-tauri/src/config.rs`

**Modifications:**
- Ajouter champ `gpu_device_index: Option<u32>` pour future sélection multi-GPU
- Garder compatibilité rétro avec `#[serde(default)]`
- Valeur par défaut: `None` (utilise device 0)

**Actions:**
1. Ajouter le champ `gpu_device_index` dans struct `AppConfig` après `use_gpu`
2. Documenter le champ avec commentaire rustdoc
3. Ajouter `gpu_device_index: None` dans impl Default

---

### `src-tauri/src/stt_whisper.rs`

**Modifications Majeures - Refactoring GPU:**

**Action 1: Ajouter compteur d'échecs statique**
- Créer `static GPU_FAILURE_COUNT: AtomicU32` en haut du fichier
- Définir constante `MAX_GPU_FAILURES: u32 = 3`
- Import nécessaire: `std::sync::atomic::{AtomicU32, Ordering}`

**Action 2: Ajouter fonctions de gestion des échecs**
- Fonction `record_gpu_failure() -> u32` - Incrémente et retourne le nouveau compte
- Fonction `reset_gpu_failures()` - Remet à zéro après succès GPU
- Fonction `get_gpu_failure_count() -> u32` - Lecture du compteur
- Fonction `is_gpu_disabled_by_failures() -> bool` - True si >= MAX_GPU_FAILURES

**Action 3: Modifier struct WhisperSttEngine**
- Ajouter champ `gpu_device_index: Option<u32>`
- Modifier signature `new()` pour accepter `gpu_device_index: Option<u32>`

**Action 4: Modifier méthode initialize()**
- Avant d'essayer GPU, vérifier `is_gpu_disabled_by_failures()`
- Si disabled, log warning et utiliser CPU directement
- Utiliser `params.gpu_device(device_index as i32)` si Some
- En cas de succès GPU, appeler `reset_gpu_failures()`
- En cas d'échec GPU, appeler `record_gpu_failure()` AVANT le fallback
- Retourner info sur le mode utilisé (nouvelle struct `GpuInitResult`)

**Action 5: Créer struct GpuInitResult**
- Champs: `mode: String` ("gpu" ou "cpu"), `device_name: Option<String>`, `fallback_reason: Option<String>`
- Modifier `initialize()` pour retourner `Result<GpuInitResult, WhisperError>`

---

### `src-tauri/src/lib.rs`

**Modifications:**

**Action 1: Créer struct GpuStatus pour événements**
- Champs: `mode: String`, `device_name: Option<String>`, `fallback_reason: Option<String>`, `failure_count: u32`
- Dériver: `Clone, Serialize, Type`

**Action 2: Modifier fonction setup_stt_engine()**
- Récupérer le résultat `GpuInitResult` de `engine.initialize()`
- Émettre événement Tauri `gpu-status` avec les infos
- Pattern: `app_handle.emit("gpu-status", &gpu_status)`
- Passer `config.gpu_device_index` au constructeur WhisperSttEngine

**Action 3: Ajouter commande get_gpu_status()**
- Retourne le statut GPU actuel
- Lit le compteur d'échecs via `stt_whisper::get_gpu_failure_count()`
- Retourne struct `GpuStatus`

**Action 4: Ajouter commande reset_gpu_failures()**
- Appelle `stt_whisper::reset_gpu_failures()`
- Permet à l'utilisateur de réessayer GPU après corrections
- Retourne message de confirmation

**Action 5: Mettre à jour get_gpu_info()**
- Ajouter champ `failure_count` dans retour
- Ajouter champ `disabled_by_failures` boolean

---

### `src/bindings.ts`

**Note: Ce fichier est auto-généré par tauri-specta**

**Actions:**
- Exécuter `npm run tauri dev` pour régénérer après modifications Rust
- OU modifier manuellement si régénération ne fonctionne pas

**Types à vérifier/ajouter:**
- `AppConfig.gpu_device_index?: number | null`
- `GpuInfo.failure_count: number`
- `GpuInfo.disabled_by_failures: boolean`
- Nouveau type `GpuStatus` avec champs mode, device_name, fallback_reason, failure_count

---

### `src/stores/settingsStore.ts`

**Modifications:**

**Action 1: Ajouter état GPU status**
- Nouveau champ `gpuStatus: GpuStatus | null` dans interface
- Nouveau champ `gpuDeviceIndex: number | null`

**Action 2: Ajouter actions**
- `setGpuStatus: (status: GpuStatus | null) => void`
- `setGpuDeviceIndex: (index: number | null) => void`

**Action 3: Mettre à jour initialState**
- `gpuStatus: null`
- `gpuDeviceIndex: null`

**Action 4: Ajouter à partialize (persistance)**
- Ajouter `gpuDeviceIndex` pour sauvegarder la préférence

---

### `src/components/SettingsWindow.tsx`

**Modifications UI:**

**Action 1: Afficher statut GPU actif**
- Dans la carte Performance/GPU, ajouter indicateur de mode actif
- Badge coloré: Vert "GPU Active" ou Orange "CPU Mode"
- Si fallback, afficher la raison en texte discret sous le badge

**Action 2: Afficher compteur d'échecs**
- Si failure_count > 0, afficher warning "X échec(s) GPU"
- Si disabled_by_failures, afficher message "GPU désactivé après 3 échecs"
- Ajouter bouton "Réessayer GPU" qui appelle reset_gpu_failures()

**Action 3: Écouter événement gpu-status**
- Dans useEffect, ajouter listener pour événement "gpu-status"
- Mettre à jour store avec le nouveau statut
- Cleanup: unlisten dans return

**Action 4: Améliorer toggle GPU**
- Désactiver toggle si GPU disabled by failures
- Tooltip expliquant pourquoi si désactivé

**Action 5: Préparer sélecteur device GPU (futur)**
- Conditionnel: afficher seulement si multiple GPU détectés
- Pour l'instant, masqué car détection multi-GPU non implémentée

---

### `src/App.tsx` (ou composant racine)

**Modifications:**

**Action 1: Listener global pour gpu-status**
- Ajouter useEffect avec listen("gpu-status")
- Mettre à jour settingsStore.setGpuStatus()
- S'assurer que le listener est actif dès le démarrage

---

## Testing Strategy

### Tests Manuels Requis

1. **Test mode CPU par défaut**
   - Démarrer app avec use_gpu: false
   - Vérifier badge "CPU Mode" dans Settings
   - Vérifier log "[Whisper] GPU: user_pref=false..."

2. **Test activation GPU**
   - Activer toggle GPU dans Settings
   - Redémarrer app
   - Vérifier badge "GPU Active" ou "CPU Mode" (si fallback)
   - Si fallback, vérifier que raison est affichée

3. **Test compteur d'échecs**
   - Simuler échec GPU (forcer erreur dans code temporairement)
   - Vérifier que failure_count s'incrémente
   - Après 3 échecs, vérifier message "GPU désactivé"
   - Tester bouton "Réessayer GPU"

4. **Test événement gpu-status**
   - Ouvrir DevTools, onglet Console
   - Démarrer app
   - Vérifier réception événement gpu-status avec payload correct

### Tests Unitaires (optionnel)

- `stt_whisper.rs`: Test `record_gpu_failure()` et `reset_gpu_failures()`
- `stt_whisper.rs`: Test `is_gpu_disabled_by_failures()` après N échecs

---

## Documentation

### À Mettre à Jour

- **CLAUDE.md**: Ajouter section sur gestion GPU et nouvelles commandes
- **README.md** (si existe): Documenter options GPU

### Nouvelles Docs (optionnel)

- `.claude/docs/gpu-management.md`: Guide détaillé GPU pour développeurs

---

## Rollout Considerations

### Compatibilité Rétro

- `gpu_device_index: Option<u32>` avec `#[serde(default)]` = compatible avec anciens configs
- Ancien config sans ce champ sera désérialisé avec `None`

### Migration

- Aucune migration nécessaire
- Nouveaux champs ont des valeurs par défaut sûres

### Feature Flags

- Aucun feature flag nécessaire
- Fonctionnalité progressive (amélioration de l'existant)

### Breaking Changes

- Aucun breaking change côté utilisateur
- API Rust interne change (signature `WhisperSttEngine::new`)

---

## Résumé des Fichiers

| Fichier | Type de Changement | Complexité |
|---------|-------------------|------------|
| `config.rs` | Ajout champ | Faible |
| `stt_whisper.rs` | Refactoring majeur | **Élevée** |
| `lib.rs` | Nouvelles commandes + événements | Moyenne |
| `bindings.ts` | Auto-généré ou manuel | Faible |
| `settingsStore.ts` | Nouveaux états | Faible |
| `SettingsWindow.tsx` | UI améliorée | Moyenne |
| `App.tsx` | Listener événement | Faible |

---

## Prochaine Étape

Exécuter `/apex:3-execute 02-gpu-vulkan-management` pour implémenter le plan.
