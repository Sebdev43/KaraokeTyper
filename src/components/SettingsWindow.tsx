import { getVersion } from "@tauri-apps/api/app";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { confirm, message } from "@tauri-apps/plugin-dialog";
import { platform } from "@tauri-apps/plugin-os";
import { relaunch } from "@tauri-apps/plugin-process";
import {
  Brain,
  Clock,
  Cpu,
  Download,
  Info,
  Keyboard,
  Maximize2,
  Mic,
  Minimize2,
  Monitor,
  RefreshCw,
  Settings,
  SpellCheck,
  Volume2,
  VolumeX,
  X,
} from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { type AppConfig, commands, type OrphanedModel, type ShortcutActivation } from "@/bindings";
import { UpdateChecker } from "@/components/UpdateChecker";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Progress } from "@/components/ui/progress";
import { ScrollArea } from "@/components/ui/scroll-area";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Separator } from "@/components/ui/separator";
import { Slider } from "@/components/ui/slider";
import { Switch } from "@/components/ui/switch";
import { cn } from "@/lib/utils";
import { useSettingsStore, useSystemStore } from "@/stores";

// Read at call time: the OS setting can change while the window stays open.
const prefersReducedMotion = () =>
  typeof window !== "undefined" && window.matchMedia("(prefers-reduced-motion: reduce)").matches;

// Event type for model download progress (not in generated bindings)
interface DownloadProgress {
  model: string;
  downloaded_mb: number;
  total_mb: number;
  percent: number;
  status: string;
}

/** Bounds and default of the hold-or-toggle threshold, mirrored by the backend clamp. */
const HOLD_THRESHOLD = { min: 100, max: 1000, step: 50, default: 300 } as const;

const clampHoldThreshold = (ms: number) =>
  Math.min(HOLD_THRESHOLD.max, Math.max(HOLD_THRESHOLD.min, Math.round(ms)));

/** One entry per `ShortcutActivation` variant: TypeScript refuses a missing one. */
const ACTIVATION_MODES: Record<ShortcutActivation, { label: string; help: string }> = {
  Hold: {
    label: "Maintenir",
    help: "Enregistre tant que la touche est maintenue ; relâchez pour transcrire.",
  },
  Toggle: {
    label: "Basculer",
    help: "Un appui démarre l'enregistrement, le suivant l'arrête.",
  },
  HoldOrToggle: {
    label: "Maintenir ou basculer",
    help: "Appui long : enregistre tant que la touche est enfoncée. Appui bref : enregistre jusqu'au prochain appui.",
  },
};

function SettingsWindow() {
  // Settings store
  const {
    audioDevices,
    setAudioDevices,
    selectedDevice,
    setSelectedDevice,
    shortcut,
    setShortcut,
    sttBackend,
    setSttBackend,
    model,
    setModel,
    language,
    setLanguage,
    hybridEngineEnabled,
    setHybridEngineEnabled,
    hybridThresholdSecs,
    setHybridThresholdSecs,
    installedModels,
    setInstalledModels,
    downloadingModel,
    setDownloadingModel,
    downloadProgress,
    setDownloadProgress,
    autostart,
    setAutostart,
    gpuInfo,
    setGpuInfo,
    gpuStatus,
    setGpuStatus,
    useGpuAcceleration,
    setUseGpuAcceleration,
    audioFeedbackEnabled,
    setAudioFeedbackEnabled,
    audioFeedbackVolume,
    setAudioFeedbackVolume,
    modelUnloadTimeout,
    setModelUnloadTimeout,
    ollamaEnabled,
    setOllamaEnabled,
    ollamaUrl,
    setOllamaUrl,
    ollamaModel,
    setOllamaModel,
    pasteMethod,
    setPasteMethod,
    shortcutActivation,
    setShortcutActivation,
    holdThresholdMs,
    setHoldThresholdMs,
    customWords,
    setCustomWords,
    customWordsThreshold,
    setCustomWordsThreshold,
    fillerRemovalEnabled,
    setFillerRemovalEnabled,
    stutterCollapseEnabled,
    setStutterCollapseEnabled,
    ollamaStatus,
    setOllamaStatus,
    isRefreshing,
    setIsRefreshing,
    saveStatus,
    setSaveStatus,
  } = useSettingsStore();

  // System store
  const {
    currentPlatform,
    setPlatform,
    permissionsInstalled,
    setPermissionsInstalled,
    uinputAvailable,
    setUinputAvailable,
    gnomeExtInstalled,
    setGnomeExtInstalled,
    gnomeExtEnabled,
    setGnomeExtEnabled,
    isGnomeWayland,
    setIsGnomeWayland,
    isInstallingSystem,
    setIsInstallingSystem,
    isInstallingGnomeExt,
    setIsInstallingGnomeExt,
    isTogglingAutostart,
    setIsTogglingAutostart,
  } = useSystemStore();

  // Local UI state
  const [isCapturing, setIsCapturing] = useState(false);
  const [isSaving, setIsSaving] = useState(false);
  const [activeSection, setActiveSection] = useState("audio");
  const inputRef = useRef<HTMLInputElement>(null);
  // The shortcut in place before a capture started, restored on Escape or blur so a
  // stray click never replaces the user's hotkey with a default.
  const previousShortcutRef = useRef("");
  // The device name the backend actually records with. The list of devices loads in
  // parallel with the settings, so the match happens once both are here (effect below);
  // selecting by stored index alone picks a different microphone whenever the list order
  // changes, and Save then silently rewrites the config with that wrong device.
  const [configuredDeviceName, setConfiguredDeviceName] = useState<string | null>(null);
  // What the backend has loaded. Save compares against it to tell the user when it is
  // about to reload a multi-GB model rather than just write a file.
  const loadedEngineRef = useRef<{
    model: string;
    language: string;
    sttBackend: string;
    useGpu: boolean;
  } | null>(null);
  const sectionRefs = useRef<Record<string, HTMLDivElement | null>>({});
  const lastVolumeRef = useRef(0.7);
  // key_delay_ms has no control in this window, so its value has to be round-tripped:
  // saving a literal would silently overwrite whatever the backend holds, and fight the
  // migration in config.rs on every save. Default mirrors DEFAULT_KEY_DELAY_MS there.
  const keyDelayMsRef = useRef(60);

  // Detect platform on mount
  useEffect(() => {
    try {
      const p = platform();
      setPlatform(p as "linux" | "windows" | "macos" | "unknown");
    } catch {
      setPlatform("linux");
    }
  }, [setPlatform]);

  // The running binary's version (tauri.conf.json), so « À propos » cannot drift
  // from a release the way a literal did.
  const [appVersion, setAppVersion] = useState<string | null>(null);
  useEffect(() => {
    getVersion()
      .then(setAppVersion)
      .catch(() => setAppVersion(null));
  }, []);

  const loadAudioDevices = useCallback(async () => {
    try {
      const result = await commands.getAudioDevices();
      if (result.status === "ok") {
        setAudioDevices(result.data);
      } else {
        console.error("Failed to load audio devices:", result.error);
      }
    } catch (error) {
      console.error("Failed to load audio devices:", error);
    }
  }, [setAudioDevices]);

  const loadSettings = useCallback(async () => {
    try {
      const result = await commands.loadSettings();
      if (result.status === "ok") {
        setShortcut(result.data.shortcut);
        keyDelayMsRef.current = result.data.key_delay_ms;
        setSelectedDevice(result.data.audio_device_index, result.data.audio_device_name ?? "");
        setConfiguredDeviceName(result.data.audio_device_name ?? null);
        setSttBackend(result.data.stt_backend ?? "whisper");
        setModel(result.data.model);
        setLanguage(result.data.language);
        setHybridEngineEnabled(result.data.hybrid_engine_enabled ?? false);
        setHybridThresholdSecs(result.data.hybrid_threshold_secs ?? 20);
        setUseGpuAcceleration(result.data.use_gpu ?? false);
        setAudioFeedbackEnabled(result.data.audio_feedback_enabled ?? true);
        setAudioFeedbackVolume(result.data.audio_feedback_volume ?? 0.7);
        setModelUnloadTimeout(result.data.model_unload_timeout ?? "Never");
        setOllamaEnabled(result.data.ollama_enabled ?? false);
        setOllamaUrl(result.data.ollama_url ?? "http://localhost:11434");
        setOllamaModel(result.data.ollama_model ?? "mistral");
        setPasteMethod(result.data.paste_method ?? "Direct");
        setShortcutActivation(result.data.shortcut_activation ?? "Hold");
        setHoldThresholdMs(
          clampHoldThreshold(result.data.hold_threshold_ms ?? HOLD_THRESHOLD.default),
        );
        // No control in this window for these four: without the round-trip, Save would
        // overwrite whatever the backend holds with the store defaults.
        setCustomWords(result.data.custom_words ?? []);
        setCustomWordsThreshold(result.data.custom_words_threshold ?? 0.3);
        setFillerRemovalEnabled(result.data.filler_removal_enabled ?? true);
        setStutterCollapseEnabled(result.data.stutter_collapse_enabled ?? true);
        loadedEngineRef.current = {
          model: result.data.model,
          language: result.data.language,
          sttBackend: result.data.stt_backend ?? "whisper",
          useGpu: result.data.use_gpu ?? false,
        };
      } else {
        console.error("Failed to load settings:", result.error);
      }
    } catch (error) {
      console.error("Failed to load settings:", error);
    }
  }, [
    setShortcut,
    setSelectedDevice,
    setSttBackend,
    setModel,
    setLanguage,
    setHybridEngineEnabled,
    setHybridThresholdSecs,
    setUseGpuAcceleration,
    setAudioFeedbackEnabled,
    setAudioFeedbackVolume,
    setModelUnloadTimeout,
    setOllamaEnabled,
    setOllamaUrl,
    setOllamaModel,
    setPasteMethod,
    setShortcutActivation,
    setHoldThresholdMs,
    setCustomWords,
    setCustomWordsThreshold,
    setFillerRemovalEnabled,
    setStutterCollapseEnabled,
  ]);

  // Match the configured microphone by name once the device list is in, then forget the
  // name so a later refresh of the list does not override a choice the user just made.
  useEffect(() => {
    if (configuredDeviceName === null || audioDevices.length === 0) return;
    const match = audioDevices.find((d) => d.raw_name === configuredDeviceName);
    if (match) setSelectedDevice(match.index, match.raw_name);
    setConfiguredDeviceName(null);
  }, [audioDevices, configuredDeviceName, setSelectedDevice]);

  const checkPermissions = useCallback(async () => {
    try {
      // Returns boolean directly (no Result wrapper)
      const installed = await commands.checkEvdevPermissions();
      setPermissionsInstalled(installed);
    } catch (error) {
      console.error("Failed to check permissions:", error);
    }
  }, [setPermissionsInstalled]);

  const checkUinput = useCallback(async () => {
    try {
      // Returns boolean directly (no Result wrapper)
      const available = await commands.checkUinputAvailable();
      setUinputAvailable(available);
    } catch (error) {
      console.error("Failed to check uinput:", error);
    }
  }, [setUinputAvailable]);

  // Files left in the models folder that no catalogue entry claims: a model from
  // a previous format, an interrupted download. Shown so the space can be
  // reclaimed on purpose — nothing here is ever deleted automatically.
  const [orphanedModels, setOrphanedModels] = useState<OrphanedModel[]>([]);
  const loadOrphanedModels = useCallback(async () => {
    try {
      setOrphanedModels(await commands.listOrphanedModels());
    } catch (_error) {
      setOrphanedModels([]);
    }
  }, []);

  const removeOrphanedModel = useCallback(
    async (name: string) => {
      const result = await commands.deleteOrphanedModel(name);
      if (result.status === "error") {
        setSaveStatus(`Suppression impossible : ${result.error}`);
        return;
      }
      await loadOrphanedModels();
    },
    [loadOrphanedModels, setSaveStatus],
  );

  const loadInstalledModels = useCallback(async () => {
    try {
      const result = await commands.getInstalledModels();
      if (result.status === "ok") {
        setInstalledModels(result.data);
      }
    } catch (_error) {
      // Silent fail - models will show as empty
    }
  }, [setInstalledModels]);

  const checkGnomeExtension = useCallback(async () => {
    try {
      const result = await commands.checkFocusedWindowExtension();
      if (result.status === "ok") {
        setGnomeExtInstalled(result.data.installed);
        setGnomeExtEnabled(result.data.enabled);
        setIsGnomeWayland(result.data.is_gnome && result.data.is_wayland);
      } else {
        console.error("Failed to check GNOME extension:", result.error);
      }
    } catch (error) {
      console.error("Failed to check GNOME extension:", error);
    }
  }, [setGnomeExtInstalled, setGnomeExtEnabled, setIsGnomeWayland]);

  const loadAutostart = useCallback(async () => {
    try {
      const enabled = await commands.getAutostartEnabled();
      setAutostart(enabled);
    } catch (error) {
      console.error("Failed to load autostart status:", error);
    }
  }, [setAutostart]);

  const loadGpuInfo = useCallback(async () => {
    try {
      const result = await commands.getGpuInfo();
      if (result.status === "ok") {
        setGpuInfo(result.data);
      }
    } catch (error) {
      console.error("Failed to load GPU info:", error);
    }
  }, [setGpuInfo]);

  const loadGpuStatus = useCallback(async () => {
    try {
      const result = await commands.getGpuStatus();
      if (result.status === "ok") {
        setGpuStatus(result.data);
      }
    } catch (error) {
      console.error("Failed to load GPU status:", error);
    }
  }, [setGpuStatus]);

  const checkOllama = useCallback(async () => {
    try {
      const result = await commands.checkOllamaStatus();
      if (result.status === "ok") {
        setOllamaStatus(result.data);
      }
    } catch (_error) {
      setOllamaStatus({ available: false, model: ollamaModel, url: ollamaUrl });
    }
  }, [ollamaUrl, ollamaModel, setOllamaStatus]);

  const handleResetGpuFailures = async () => {
    try {
      const result = await commands.resetGpuFailureCount();
      if (result.status === "ok") {
        await message(result.data, { title: "GPU Reset", kind: "info" });
        // Reload GPU info and status to reflect changes
        await loadGpuInfo();
        await loadGpuStatus();
      } else {
        console.error("Failed to reset GPU failures:", result.error);
      }
    } catch (error) {
      console.error("Failed to reset GPU failures:", error);
    }
  };

  const handleToggleAutostart = async (enabled: boolean) => {
    setIsTogglingAutostart(true);
    try {
      const result = await commands.setAutostartEnabled(enabled);
      if (result.status === "ok") {
        setAutostart(enabled);
      } else {
        console.error("Failed to toggle autostart:", result.error);
        await message(`Échec du changement d'autostart : ${result.error}`, {
          title: "Erreur",
          kind: "error",
        });
      }
    } catch (error) {
      console.error("Failed to toggle autostart:", error);
      await message(`Échec du changement d'autostart : ${error}`, {
        title: "Erreur",
        kind: "error",
      });
    } finally {
      setIsTogglingAutostart(false);
    }
  };

  const handleDownloadModel = async (modelName: string) => {
    const modelInfo = installedModels.find((m) => m.name === modelName);
    if (!modelInfo) return;

    const confirmed = await confirm(
      `Télécharger le modèle ${modelName} (${modelInfo.size_mb} MB) ?\n\nCela peut prendre plusieurs minutes selon votre connexion.`,
      { title: "Télécharger un modèle", kind: "info" },
    );

    if (!confirmed) return;

    setDownloadingModel(modelName);
    setDownloadProgress(0);

    try {
      const result = await commands.downloadModel(modelName);
      if (result.status === "ok") {
        // Show 100% complete for 1.5 seconds
        setDownloadProgress(100);
        await new Promise((r) => setTimeout(r, 1500));
        await loadInstalledModels();
      } else {
        await message(`Échec du téléchargement : ${result.error}`, {
          title: "Erreur",
          kind: "error",
        });
      }
    } catch (error) {
      await message(`Échec du téléchargement : ${error}`, {
        title: "Erreur",
        kind: "error",
      });
    } finally {
      setDownloadingModel(null);
      setDownloadProgress(0);
    }
  };

  useEffect(() => {
    void loadAudioDevices();
    void loadSettings();
    void checkPermissions();
    void checkUinput();
    void loadInstalledModels();
    void loadOrphanedModels();
    void checkGnomeExtension();
    void loadAutostart();
    void loadGpuInfo();
    void loadGpuStatus();
  }, [
    loadAudioDevices,
    loadSettings,
    checkPermissions,
    checkUinput,
    loadInstalledModels,
    loadOrphanedModels,
    checkGnomeExtension,
    loadAutostart,
    loadGpuInfo,
    loadGpuStatus,
  ]);

  // The window is hidden, never destroyed: edits abandoned with the close button or
  // Alt+F4 would otherwise still be on screen the next time it is shown, looking saved.
  useEffect(() => {
    const onVisibility = () => {
      if (document.visibilityState !== "visible") return;
      void loadSettings();
      void loadInstalledModels();
    };
    document.addEventListener("visibilitychange", onVisibility);
    return () => document.removeEventListener("visibilitychange", onVisibility);
  }, [loadSettings, loadInstalledModels]);

  useEffect(() => {
    const unlistenProgress = listen<DownloadProgress>("model-download-progress", (event) => {
      setDownloadProgress(event.payload.percent);
    });

    const unlistenComplete = listen("model-download-complete", () => {
      // Refresh models list when download completes
      void loadInstalledModels();
    });

    return () => {
      void unlistenProgress.then((fn) => fn());
      void unlistenComplete.then((fn) => fn());
    };
  }, [loadInstalledModels, setDownloadProgress]);

  const startShortcutCapture = () => {
    if (isCapturing) return;
    previousShortcutRef.current = shortcut;
    setIsCapturing(true);
    inputRef.current?.focus();
  };

  const cancelShortcutCapture = () => {
    if (!isCapturing) return;
    setShortcut(previousShortcutRef.current);
    setIsCapturing(false);
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (!isCapturing) {
      // Keyboard users reach the field with Tab; Enter or Space starts the capture.
      if (e.key === "Enter" || e.key === " ") {
        e.preventDefault();
        startShortcutCapture();
      }
      return;
    }

    // Escape cancels; Tab and Enter are never a hotkey (Tab moves focus, the blur cancels).
    // stopPropagation keeps the window-level Escape (close) from seeing the same event:
    // React re-renders synchronously here, so that listener would already see
    // isCapturing === false by the time the native event reaches it.
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      cancelShortcutCapture();
      return;
    }
    if (e.key === "Tab" || e.key === "Enter") return;

    e.preventDefault();
    e.stopPropagation();

    const modifiers: string[] = [];
    if (e.ctrlKey) modifiers.push("Ctrl");
    if (e.altKey) modifiers.push("Alt");
    if (e.shiftKey) modifiers.push("Shift");
    if (e.metaKey) modifiers.push("Super");

    let key = e.key;

    if (key === " ") key = "Space";
    else if (key.length === 1) key = key.toUpperCase();

    if (key === "Control" || key === "Alt" || key === "Shift" || key === "Meta") return;

    const newShortcut = [...modifiers, key].join("+");
    setShortcut(newShortcut);
    setIsCapturing(false);
  };

  // Map mouse button numbers to readable names
  const getMouseButtonName = (button: number): string => {
    switch (button) {
      case 0:
        return "MouseLeft";
      case 1:
        return "MouseMiddle";
      case 2:
        return "MouseRight";
      case 3:
        return "Mouse4"; // Back button on gaming mice
      case 4:
        return "Mouse5"; // Forward button on gaming mice
      default:
        return `Mouse${button + 1}`;
    }
  };

  const handleMouseDown = (e: React.MouseEvent) => {
    if (!isCapturing) return;

    // Ignore left click (button 0) as it's used for UI interaction
    // Allow right click (2), middle click (1), and extra buttons (3, 4, etc.)
    if (e.button === 0) return;

    e.preventDefault();
    e.stopPropagation();

    const modifiers: string[] = [];
    if (e.ctrlKey) modifiers.push("Ctrl");
    if (e.altKey) modifiers.push("Alt");
    if (e.shiftKey) modifiers.push("Shift");
    if (e.metaKey) modifiers.push("Super");

    const buttonName = getMouseButtonName(e.button);
    const newShortcut = [...modifiers, buttonName].join("+");
    setShortcut(newShortcut);
    setIsCapturing(false);
  };

  const handleBlur = () => {
    cancelShortcutCapture();
  };

  const handleRefreshDevices = async () => {
    setIsRefreshing(true);
    try {
      await loadAudioDevices();
    } catch (error) {
      console.error("Failed to refresh audio devices:", error);
    } finally {
      setIsRefreshing(false);
    }
  };

  const handleSave = async () => {
    if (isSaving) return;
    setIsSaving(true);
    try {
      // save_settings reloads the engine inline when any of these changed, which takes
      // seconds for a multi-GB Whisper: say so instead of hanging on "Enregistrement…".
      const loaded = loadedEngineRef.current;
      const engineReload =
        loaded !== null &&
        (loaded.model !== model ||
          loaded.language !== language ||
          loaded.sttBackend !== sttBackend ||
          loaded.useGpu !== useGpuAcceleration);
      const modelLabel = installedModels.find((m) => m.name === model)?.display_name ?? model;
      setSaveStatus(
        engineReload
          ? `Enregistrement, puis chargement de ${modelLabel} (quelques secondes)…`
          : "Enregistrement…",
      );

      const selectedDeviceName =
        audioDevices.find((d) => d.index === selectedDevice)?.raw_name || null;

      // Get model path for the selected model
      const selectedModel = installedModels.find((m) => m.name === model);
      const modelPath = selectedModel?.installed ? selectedModel.url : null;

      const config: AppConfig = {
        shortcut,
        audio_device_index: selectedDevice,
        audio_device_name: selectedDeviceName,
        model,
        key_delay_ms: keyDelayMsRef.current,
        language,
        model_path: modelPath,
        use_gpu: useGpuAcceleration,
        audio_feedback_enabled: audioFeedbackEnabled,
        audio_feedback_volume: audioFeedbackVolume,
        model_unload_timeout: modelUnloadTimeout,
        ollama_enabled: ollamaEnabled,
        ollama_url: ollamaUrl,
        ollama_model: ollamaModel,
        paste_method: pasteMethod,
        custom_words: customWords,
        custom_words_threshold: customWordsThreshold,
        filler_removal_enabled: fillerRemovalEnabled,
        stutter_collapse_enabled: stutterCollapseEnabled,
        stt_backend: sttBackend,
        shortcut_activation: shortcutActivation,
        hold_threshold_ms: holdThresholdMs,
        hybrid_engine_enabled: hybridEngineEnabled,
        hybrid_threshold_secs: hybridThresholdSecs,
      };

      const result = await commands.saveSettings(config);
      if (result.status === "ok") {
        loadedEngineRef.current = {
          model,
          language,
          sttBackend,
          useGpu: useGpuAcceleration,
        };
        setSaveStatus(
          engineReload ? `Enregistré, ${modelLabel} chargé.` : "Paramètres enregistrés.",
        );
        // Leave the confirmation on screen long enough to be read before the window goes.
        await new Promise((resolve) => setTimeout(resolve, 700));
        await getCurrentWindow().hide();
        setSaveStatus("");
      } else {
        setSaveStatus(`Erreur : ${result.error}`);
        console.error("Failed to save settings:", result.error);
      }
    } catch (error) {
      setSaveStatus(`Erreur : ${error}`);
      console.error("Failed to save settings:", error);
    } finally {
      setIsSaving(false);
    }
  };

  // Drop unsaved edits: hide, then reload what the backend actually holds.
  const handleCancel = async () => {
    if (isSaving) return;
    setSaveStatus("");
    await getCurrentWindow().hide();
    void loadSettings();
  };

  const handleInstallSystem = async () => {
    const confirmed = await confirm(
      "Configurer les permissions système ?\n\nCela va :\n• Configurer evdev pour le push-to-talk\n• Installer xdotool (saisie de texte)\n• Installer xclip (clipboard pour RustDesk/Remmina)\n• Peut nécessiter le redémarrage de l'application",
      { title: "Configuration KaraokeTyper", kind: "warning" },
    );

    if (!confirmed) return;

    setIsInstallingSystem(true);

    try {
      const result = await commands.installSystemDependencies();

      if (result.status === "ok") {
        if (result.data.includes("restart")) {
          await relaunch();
        } else {
          await message(result.data, { title: "Succès", kind: "info" });
          await checkPermissions();
          await checkUinput();
        }
      } else {
        await message(`Échec de l'installation : ${result.error}`, {
          title: "Erreur",
          kind: "error",
        });
      }
    } catch (error) {
      await message(`Échec de l'installation : ${error}`, {
        title: "Erreur",
        kind: "error",
      });
    } finally {
      setIsInstallingSystem(false);
    }
  };

  const handleInstallGnomeExtension = async () => {
    const confirmed = await confirm(
      "Installer l'extension GNOME pour la détection de terminal ?\n\nCela permet de coller automatiquement avec Ctrl+Shift+V dans les terminaux.\n\nNote: La session sera redémarrée automatiquement après l'installation.",
      { title: "Extension Terminal", kind: "info" },
    );

    if (!confirmed) return;

    setIsInstallingGnomeExt(true);

    try {
      const result = await commands.installFocusedWindowExtension();
      if (result.status === "ok") {
        await message(result.data, { title: "Succès", kind: "info" });
      } else {
        await message(`Échec de l'installation : ${result.error}`, {
          title: "Erreur",
          kind: "error",
        });
      }
    } catch (error) {
      await message(`Échec de l'installation : ${error}`, {
        title: "Erreur",
        kind: "error",
      });
    } finally {
      setIsInstallingGnomeExt(false);
    }
  };

  const handleRemoveGnomeExtension = async () => {
    const confirmed = await confirm("Supprimer l'extension de détection de terminal ?", {
      title: "Supprimer Extension",
      kind: "warning",
    });

    if (!confirmed) return;

    setIsInstallingGnomeExt(true);

    try {
      const result = await commands.removeFocusedWindowExtension();
      if (result.status === "ok") {
        await message(result.data, { title: "Succès", kind: "info" });
        await checkGnomeExtension();
      } else {
        await message(`Échec de la suppression : ${result.error}`, {
          title: "Erreur",
          kind: "error",
        });
      }
    } catch (error) {
      await message(`Échec de la suppression : ${error}`, {
        title: "Erreur",
        kind: "error",
      });
    } finally {
      setIsInstallingGnomeExt(false);
    }
  };

  const systemDepsInstalled = permissionsInstalled && uinputAvailable;

  // Hybrid mode only makes sense with Whisper as primary: `model` is one field, so a
  // Parakeet primary has no Whisper model to hand long dictations to.
  const hybridAvailable = sttBackend === "whisper";
  const parakeetInstalled = installedModels.some(
    (m) => m.engine_type === "parakeet" && m.installed,
  );

  const handleMinimize = async () => {
    await getCurrentWindow().minimize();
  };

  const handleMaximize = async () => {
    const win = getCurrentWindow();
    if (await win.isMaximized()) {
      await win.unmaximize();
    } else {
      await win.maximize();
    }
  };

  const handleClose = async () => {
    await handleCancel();
  };

  // Ctrl+S saves, Escape cancels, unless a capture or an open menu owns the key.
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (isCapturing || e.defaultPrevented) return;
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "s") {
        e.preventDefault();
        void handleSave();
        return;
      }
      if (e.key === "Escape" && !document.querySelector("[data-radix-popper-content-wrapper]")) {
        e.preventDefault();
        void handleCancel();
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  });

  return (
    <div className="flex flex-col h-screen overflow-hidden bg-background">
      {/* Custom Titlebar - needed for Wayland compatibility */}
      <div
        data-tauri-drag-region
        className="flex-none flex items-center justify-between h-10 px-3 border-b border-border bg-muted/50 select-none"
      >
        <div data-tauri-drag-region className="flex items-center gap-2">
          <Settings className="h-4 w-4 text-primary" />
          <h1 className="text-sm font-medium">KaraokeTyper · Paramètres</h1>
        </div>
        <div className="flex items-center gap-1">
          <Button
            variant="ghost"
            size="icon"
            className="h-7 w-7 hover:bg-muted"
            onClick={handleMinimize}
            aria-label="Réduire la fenêtre"
          >
            <Minimize2 className="h-3.5 w-3.5" />
          </Button>
          <Button
            variant="ghost"
            size="icon"
            className="h-7 w-7 hover:bg-muted"
            onClick={handleMaximize}
            aria-label="Agrandir la fenêtre"
          >
            <Maximize2 className="h-3.5 w-3.5" />
          </Button>
          <Button
            variant="ghost"
            size="icon"
            className="h-7 w-7 hover:bg-destructive hover:text-destructive-foreground"
            onClick={handleClose}
            aria-label="Fermer sans enregistrer"
          >
            <X className="h-3.5 w-3.5" />
          </Button>
        </div>
      </div>

      {/* Sidebar + Content */}
      <div className="flex flex-1 min-h-0">
        {/* Sidebar Navigation */}
        <nav
          aria-label="Sections des paramètres"
          className="flex-none w-48 border-r border-border bg-muted/30 py-2 overflow-y-auto"
        >
          {[
            { id: "audio", icon: Mic, label: "Audio" },
            {
              id: "shortcut",
              icon: Keyboard,
              label: "Raccourci",
            },
            {
              id: "stt",
              icon: Brain,
              label: "Reconnaissance",
            },
            {
              id: "performance",
              icon: Cpu,
              label: "Performance",
            },
            {
              id: "models",
              icon: Download,
              label: "Modèles",
            },
            {
              id: "system",
              icon: Settings,
              label: "Système",
            },
            ...(isGnomeWayland
              ? [
                  {
                    id: "gnome",
                    icon: Monitor,
                    label: "Terminal",
                  },
                ]
              : []),
            {
              id: "feedback",
              icon: Volume2,
              label: "Sons",
            },
            {
              id: "unload",
              icon: Clock,
              label: "Déchargement",
            },
            {
              id: "ollama",
              icon: SpellCheck,
              label: "Ollama",
            },
            {
              id: "about",
              icon: Info,
              label: "À propos",
            },
          ].map(({ id, icon: Icon, label }) => (
            <button
              key={id}
              type="button"
              onClick={() => {
                setActiveSection(id);
                sectionRefs.current[id]?.scrollIntoView({
                  behavior: prefersReducedMotion() ? "auto" : "smooth",
                  block: "start",
                });
              }}
              aria-current={activeSection === id ? "true" : undefined}
              className={cn(
                "flex items-center gap-2.5 w-full px-4 py-2 text-sm transition-colors text-left",
                "focus-visible:outline-none focus-visible:ring-[3px] focus-visible:ring-inset focus-visible:ring-ring/50",
                activeSection === id
                  ? "bg-primary/10 text-foreground font-medium border-r-2 border-primary"
                  : "text-muted-foreground hover:text-foreground hover:bg-muted/50",
              )}
            >
              <Icon
                className={cn(
                  "h-4 w-4 shrink-0",
                  activeSection === id ? "text-foreground" : "text-muted-foreground",
                )}
              />
              {label}
            </button>
          ))}
        </nav>

        {/* Content */}
        <ScrollArea className="flex-1 min-h-0">
          <div className="p-6 space-y-6">
            {/* Audio Input */}
            <Card
              ref={(el) => {
                sectionRefs.current.audio = el;
              }}
            >
              <CardHeader className="pb-3">
                <CardTitle className="flex items-center gap-2 text-lg">
                  <Mic className="h-5 w-5 text-muted-foreground" />
                  Entrée audio
                </CardTitle>
              </CardHeader>
              <CardContent className="space-y-4">
                <div className="space-y-2">
                  <Label htmlFor="microphone">Microphone</Label>
                  <div className="flex gap-2">
                    <Select
                      value={String(selectedDevice)}
                      onValueChange={(value) => setSelectedDevice(Number(value))}
                    >
                      <SelectTrigger id="microphone" className="flex-1">
                        <SelectValue placeholder="Sélectionner un micro" />
                      </SelectTrigger>
                      <SelectContent className="max-w-[calc(100vw-15rem)]">
                        {audioDevices.map((device) => (
                          <SelectItem
                            key={device.index}
                            value={String(device.index)}
                            title={device.name}
                          >
                            <span className="truncate">{device.name}</span>
                          </SelectItem>
                        ))}
                      </SelectContent>
                    </Select>
                    <Button
                      variant="outline"
                      size="icon"
                      onClick={handleRefreshDevices}
                      disabled={isRefreshing}
                      aria-label="Actualiser la liste des micros"
                    >
                      <RefreshCw
                        className={cn(
                          "h-4 w-4",
                          isRefreshing && "animate-spin motion-reduce:animate-none",
                        )}
                      />
                    </Button>
                  </div>
                </div>
              </CardContent>
            </Card>

            {/* Keyboard Shortcut */}
            <Card
              ref={(el) => {
                sectionRefs.current.shortcut = el;
              }}
            >
              <CardHeader className="pb-3">
                <CardTitle className="flex items-center gap-2 text-lg">
                  <Keyboard className="h-5 w-5 text-muted-foreground" />
                  Raccourci clavier
                </CardTitle>
              </CardHeader>
              <CardContent className="space-y-4">
                <div className="space-y-2">
                  <Label htmlFor="shortcut">Touche d'enregistrement</Label>
                  <div className="flex gap-2">
                    <Input
                      ref={inputRef}
                      id="shortcut"
                      type="text"
                      value={isCapturing ? "" : shortcut}
                      onClick={startShortcutCapture}
                      onKeyDown={handleKeyDown}
                      onMouseDown={handleMouseDown}
                      onBlur={handleBlur}
                      onContextMenu={(e) => e.preventDefault()}
                      readOnly
                      aria-describedby="shortcut-hint"
                      placeholder={
                        isCapturing ? "Appuyez sur une touche ou un bouton souris…" : "Non défini"
                      }
                      className={cn(
                        "flex-1 cursor-pointer",
                        isCapturing && "ring-[3px] ring-ring/50 border-ring bg-primary/10",
                      )}
                    />
                    <Button
                      type="button"
                      variant="outline"
                      onClick={isCapturing ? cancelShortcutCapture : startShortcutCapture}
                      onMouseDown={handleMouseDown}
                    >
                      {isCapturing ? "Annuler" : "Modifier"}
                    </Button>
                  </div>
                  <p
                    id="shortcut-hint"
                    className="text-xs text-muted-foreground"
                    aria-live="polite"
                  >
                    {isCapturing
                      ? "En attente d'une touche ou d'un bouton souris (Mouse4, Mouse5…). Échap pour annuler."
                      : "Cliquez sur Modifier, puis appuyez sur la combinaison voulue."}
                  </p>
                </div>

                {/* Shortcut activation */}
                <div className="space-y-2">
                  <Label htmlFor="shortcut-activation">Mode d'activation</Label>
                  <Select
                    value={shortcutActivation}
                    onValueChange={(v) => setShortcutActivation(v as ShortcutActivation)}
                  >
                    <SelectTrigger id="shortcut-activation">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      {(Object.keys(ACTIVATION_MODES) as ShortcutActivation[]).map((mode) => (
                        <SelectItem key={mode} value={mode}>
                          {ACTIVATION_MODES[mode].label}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                  <p className="text-xs text-muted-foreground">
                    {ACTIVATION_MODES[shortcutActivation].help}
                  </p>
                </div>

                {shortcutActivation === "HoldOrToggle" && (
                  <div className="space-y-2">
                    <Label htmlFor="hold-threshold">Seuil de maintien ({holdThresholdMs} ms)</Label>
                    <Slider
                      id="hold-threshold"
                      min={HOLD_THRESHOLD.min}
                      max={HOLD_THRESHOLD.max}
                      step={HOLD_THRESHOLD.step}
                      value={[holdThresholdMs]}
                      onValueChange={([v]) => setHoldThresholdMs(v)}
                    />
                    <p className="text-xs text-muted-foreground">
                      Relâché avant ce délai, l'appui est une bascule ; après, un maintien (
                      {HOLD_THRESHOLD.default} ms par défaut).
                    </p>
                  </div>
                )}
              </CardContent>
            </Card>

            {/* Speech Recognition */}
            <Card
              ref={(el) => {
                sectionRefs.current.stt = el;
              }}
            >
              <CardHeader className="pb-3">
                <CardTitle className="flex items-center gap-2 text-lg">
                  <Brain className="h-5 w-5 text-muted-foreground" />
                  Reconnaissance vocale
                </CardTitle>
              </CardHeader>
              <CardContent className="space-y-4">
                <div className="space-y-2">
                  <Label htmlFor="stt-backend">Moteur STT (auto-hébergé)</Label>
                  <Select
                    value={sttBackend}
                    onValueChange={(value) => {
                      const next = value as typeof sttBackend;
                      setSttBackend(next);
                      const first = installedModels.find(
                        (m) => (m.engine_type ?? "whisper") === next && m.installed,
                      );
                      if (first) setModel(first.name);
                    }}
                  >
                    <SelectTrigger id="stt-backend">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="whisper">Whisper · généraliste, 99 langues</SelectItem>
                      <SelectItem value="parakeet">
                        Parakeet V3 · FR + 24 langues UE, très précis (recommandé)
                      </SelectItem>
                      <SelectItem value="sense-voice" disabled>
                        SenseVoice · multilingue (FR/EN/ES/DE), bientôt
                      </SelectItem>
                      <SelectItem value="moonshine" disabled>
                        Moonshine · ultra-léger (EN), bientôt
                      </SelectItem>
                    </SelectContent>
                  </Select>
                  <p className="text-xs text-muted-foreground">
                    100 % local. Parakeet V3 auto-détecte la langue dans son set de 25. Whisper
                    accepte une langue explicite.
                  </p>
                </div>

                <div className="space-y-2">
                  <Label htmlFor="model">Modèle actif</Label>
                  <Select value={model} onValueChange={setModel}>
                    <SelectTrigger id="model">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      {installedModels
                        .filter((m) => (m.engine_type ?? "whisper") === sttBackend)
                        .map((modelInfo) => (
                          <SelectItem
                            key={modelInfo.name}
                            value={modelInfo.name}
                            disabled={!modelInfo.installed || modelInfo.coming_soon}
                          >
                            {modelInfo.display_name} - {modelInfo.size_mb} MB
                            {modelInfo.coming_soon && " (bientôt)"}
                            {!modelInfo.installed && !modelInfo.coming_soon && " (Non installé)"}
                          </SelectItem>
                        ))}
                    </SelectContent>
                  </Select>
                  {model && !installedModels.find((m) => m.name === model)?.installed && (
                    <p className="text-xs text-destructive">
                      Le modèle "{model}" n'est pas installé. Téléchargez-le dans "Modèles"
                      ci-dessous.
                    </p>
                  )}
                  {/* The 1.9.0 format change leaves the previous .bin behind, and it is big.
                      Say so here, where the user discovers the model is missing, instead of
                      only in the engine log nobody reads. */}
                  {orphanedModels.length > 0 && (
                    <p className="text-xs text-muted-foreground">
                      {orphanedModels.length === 1
                        ? "Un ancien fichier de modèle occupe encore"
                        : `${orphanedModels.length} anciens fichiers de modèle occupent encore`}{" "}
                      {orphanedModels.reduce((sum, o) => sum + o.size_mb, 0)} Mo : l'encart
                      «&nbsp;Fichiers inutilisés&nbsp;» de la section «&nbsp;Modèles&nbsp;»
                      ci-dessous permet de les supprimer.
                    </p>
                  )}
                </div>

                {/* Hybrid mode: Parakeet for short dictations, Whisper beyond the threshold */}
                <div className="space-y-2">
                  <div className="flex items-center justify-between">
                    <div className="space-y-1">
                      <Label htmlFor="hybrid-engine">Parakeet pour les dictées courtes</Label>
                      <p className="text-sm text-muted-foreground">
                        Parakeet V3 est bien plus rapide que Whisper : il transcrit les dictées
                        courtes, Whisper reprend la main au-delà du seuil.
                      </p>
                    </div>
                    <Switch
                      id="hybrid-engine"
                      checked={hybridEngineEnabled}
                      onCheckedChange={setHybridEngineEnabled}
                      disabled={!hybridAvailable}
                    />
                  </div>
                  {!hybridAvailable && (
                    <p className="text-xs text-muted-foreground">
                      Disponible uniquement avec Whisper comme moteur principal.
                    </p>
                  )}
                  {hybridAvailable && hybridEngineEnabled && !parakeetInstalled && (
                    <p className="text-xs text-destructive">
                      Parakeet V3 n'est pas installé : toutes les dictées passent par Whisper
                      jusqu'à son téléchargement dans "Modèles" ci-dessous.
                    </p>
                  )}
                  {hybridAvailable && hybridEngineEnabled && (
                    <div className="space-y-2 pt-1">
                      <Label id="hybrid-threshold-label">
                        Bascule vers Whisper au-delà de {hybridThresholdSecs} s
                      </Label>
                      <Slider
                        aria-labelledby="hybrid-threshold-label"
                        min={3}
                        max={120}
                        step={1}
                        value={[hybridThresholdSecs]}
                        onValueChange={([v]) => setHybridThresholdSecs(v)}
                      />
                    </div>
                  )}
                </div>

                <div className="space-y-2">
                  <Label htmlFor="language">Langue</Label>
                  <Select value={language} onValueChange={setLanguage}>
                    <SelectTrigger id="language">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="fr">Français (French)</SelectItem>
                      <SelectItem value="en">English</SelectItem>
                      <SelectItem value="es">Español (Spanish)</SelectItem>
                      <SelectItem value="de">Deutsch (German)</SelectItem>
                      <SelectItem value="it">Italiano (Italian)</SelectItem>
                      <SelectItem value="pt">Português (Portuguese)</SelectItem>
                      <SelectItem value="nl">Nederlands (Dutch)</SelectItem>
                      <SelectItem value="pl">Polski (Polish)</SelectItem>
                      <SelectItem value="ru">Русский (Russian)</SelectItem>
                      <SelectItem value="zh">中文 (Chinese)</SelectItem>
                      <SelectItem value="ja">日本語 (Japanese)</SelectItem>
                      <SelectItem value="ko">한국어 (Korean)</SelectItem>
                    </SelectContent>
                  </Select>
                </div>
              </CardContent>
            </Card>

            {/* Performance / GPU Acceleration */}
            <Card
              ref={(el) => {
                sectionRefs.current.performance = el;
              }}
            >
              <CardHeader className="pb-3">
                <CardTitle className="flex items-center gap-2 text-lg">
                  <Cpu className="h-5 w-5 text-muted-foreground" />
                  Performance
                </CardTitle>
              </CardHeader>
              <CardContent className="space-y-4">
                {/* GPU Info */}
                <div className="flex items-center justify-between">
                  <div className="space-y-1">
                    <Label>GPU détecté</Label>
                    <p className="text-sm text-muted-foreground">
                      {gpuInfo ? gpuInfo.device_name || "Aucun GPU détecté" : "Détection..."}
                      {gpuInfo?.vram_mb
                        ? ` (${gpuInfo.vram_mb >= 1024 ? `${(gpuInfo.vram_mb / 1024).toFixed(1)} GB` : `${gpuInfo.vram_mb} MB`} VRAM)`
                        : ""}
                    </p>
                  </div>
                  <Badge variant={gpuInfo?.available ? "default" : "secondary"}>
                    {gpuInfo ? (gpuInfo.available ? gpuInfo.backend : "CPU seul") : "..."}
                  </Badge>
                </div>

                {/* VRAM Recommendation */}
                {gpuInfo?.vram_mb && gpuInfo.recommended_model && (
                  <div className="p-2 rounded-md bg-muted/50 border border-border">
                    <p className="text-sm text-muted-foreground">
                      Modèle max recommandé pour votre GPU :{" "}
                      <span className="font-medium text-foreground">
                        {gpuInfo.recommended_model}
                      </span>
                    </p>
                    {(() => {
                      // Mirrors model_vram_mb() in src-tauri/src/gpu.rs: the GGUF
                      // file plus its compute buffers. Quantised models need far
                      // less than the GGML ones this table used to describe.
                      const vramTable: Record<string, number> = {
                        "whisper-tiny": 100,
                        "whisper-base": 150,
                        "whisper-small": 450,
                        "whisper-medium": 1300,
                        "whisper-large-v3-turbo": 1400,
                        "whisper-large-v3": 1800,
                      };
                      const currentVram = vramTable[model] ?? 0;
                      const isTooLarge =
                        gpuInfo.vram_mb !== null && currentVram > gpuInfo.vram_mb * 0.7;
                      if (useGpuAcceleration && isTooLarge) {
                        return (
                          <p className="text-xs text-destructive mt-1">
                            Votre modèle actuel ({model}) risque de dépasser la capacité VRAM et
                            provoquer des crashs. Essayez {gpuInfo.recommended_model}.
                          </p>
                        );
                      }
                      return null;
                    })()}
                  </div>
                )}

                {/* Current Mode Status */}
                {gpuStatus && (
                  <div className="flex items-center justify-between">
                    <div className="space-y-1">
                      <Label>Mode actuel</Label>
                      <p className="text-sm text-muted-foreground">
                        {gpuStatus.fallback_reason || "Fonctionne normalement"}
                      </p>
                    </div>
                    <Badge variant={gpuStatus.mode === "gpu" ? "default" : "secondary"}>
                      {gpuStatus.mode === "gpu" ? "GPU actif" : "Mode CPU"}
                    </Badge>
                  </div>
                )}

                {/* GPU Failure Warning */}
                {gpuInfo && gpuInfo.failure_count > 0 && (
                  <div className="flex items-center justify-between p-2 rounded-md bg-destructive/10 border border-destructive/30">
                    <div className="space-y-1">
                      <p className="text-sm text-destructive font-medium">
                        {gpuInfo.disabled_by_failures
                          ? `GPU désactivé après ${gpuInfo.failure_count} échecs`
                          : `${gpuInfo.failure_count} échec(s) GPU enregistré(s)`}
                      </p>
                      <p className="text-xs text-muted-foreground">
                        {gpuInfo.disabled_by_failures
                          ? "Le GPU ne sera pas utilisé tant que vous ne réinitialisez pas le compteur"
                          : "Le GPU sera désactivé après 3 échecs consécutifs"}
                      </p>
                    </div>
                    <Button
                      size="sm"
                      variant="outline"
                      onClick={handleResetGpuFailures}
                      className="shrink-0"
                    >
                      <RefreshCw className="h-4 w-4 mr-1" />
                      Réessayer
                    </Button>
                  </div>
                )}

                <Separator />

                {/* GPU Toggle */}
                <div className="flex items-center justify-between">
                  <div className="space-y-1">
                    <Label htmlFor="gpu-acceleration">Accélération GPU</Label>
                    <p className="text-sm text-muted-foreground">
                      {useGpuAcceleration
                        ? "GPU utilisé pour la transcription"
                        : "CPU utilisé (plus compatible)"}
                    </p>
                  </div>
                  <Switch
                    id="gpu-acceleration"
                    checked={useGpuAcceleration}
                    onCheckedChange={setUseGpuAcceleration}
                    disabled={!gpuInfo?.available || gpuInfo?.disabled_by_failures}
                  />
                </div>

                {!gpuInfo?.available && (
                  <p className="text-xs text-muted-foreground">
                    Accélération GPU non disponible. Compilé sans support GPU ou aucun GPU
                    compatible détecté.
                  </p>
                )}
              </CardContent>
            </Card>

            {/* Model Management */}
            <Card
              ref={(el) => {
                sectionRefs.current.models = el;
              }}
            >
              <CardHeader className="pb-3">
                <CardTitle className="flex items-center gap-2 text-lg">
                  <Download className="h-5 w-5 text-muted-foreground" />
                  Gestion des modèles
                </CardTitle>
              </CardHeader>
              <CardContent className="space-y-3">
                <p className="text-sm text-muted-foreground">
                  Télécharger les modèles de reconnaissance vocale. Whisper est au format GGUF
                  depuis la 1.9.0 : un fichier installé avant n'est plus lisible.
                </p>

                {orphanedModels.length > 0 && (
                  <div className="rounded-lg border border-border p-3 space-y-2">
                    <div className="flex items-baseline justify-between gap-2">
                      <h3 className="text-sm font-medium">Fichiers inutilisés</h3>
                      <span className="text-xs text-muted-foreground">
                        {orphanedModels.reduce((sum, o) => sum + o.size_mb, 0)} Mo récupérables
                      </span>
                    </div>
                    <p className="text-xs text-muted-foreground">
                      Plus aucun modèle du catalogue n'utilise ces fichiers : format précédent,
                      téléchargement interrompu. Rien n'est supprimé sans votre geste.
                    </p>
                    {orphanedModels.map((orphan) => (
                      <div key={orphan.name} className="flex items-center justify-between gap-3">
                        <span className="min-w-0 truncate text-sm" title={orphan.path}>
                          {orphan.name}
                          <span className="ml-2 text-xs text-muted-foreground">
                            {orphan.size_mb} Mo
                          </span>
                        </span>
                        <Button
                          variant="outline"
                          size="sm"
                          onClick={() => void removeOrphanedModel(orphan.name)}
                        >
                          Supprimer
                        </Button>
                      </div>
                    ))}
                  </div>
                )}

                {installedModels.map((modelInfo) => (
                  <div
                    key={modelInfo.name}
                    className="flex items-center justify-between p-3 rounded-lg border border-border"
                  >
                    <div className="flex-1">
                      <div className="flex items-center gap-2">
                        <span className="font-medium">{modelInfo.display_name}</span>
                        {modelInfo.installed && (
                          <Badge variant="default" className="text-xs">
                            Installé
                          </Badge>
                        )}
                        {modelInfo.is_recommended && (
                          <Badge variant="secondary" className="text-xs">
                            Recommandé
                          </Badge>
                        )}
                        {modelInfo.coming_soon && (
                          <Badge variant="secondary" className="text-xs">
                            Bientôt
                          </Badge>
                        )}
                      </div>
                      <div className="text-sm text-muted-foreground mt-1">
                        {modelInfo.description}
                      </div>

                      {downloadingModel === modelInfo.name && (
                        <div className="mt-2 space-y-1">
                          <Progress value={downloadProgress} className="h-2" />
                          <p className="text-xs text-muted-foreground">
                            Téléchargement… {downloadProgress.toFixed(1)}%
                          </p>
                        </div>
                      )}
                    </div>

                    {!modelInfo.installed && !modelInfo.coming_soon && (
                      <Button
                        variant="outline"
                        size="sm"
                        onClick={() => handleDownloadModel(modelInfo.name)}
                        disabled={downloadingModel !== null}
                      >
                        Télécharger
                      </Button>
                    )}
                  </div>
                ))}
              </CardContent>
            </Card>

            {/* System Setup */}
            <Card
              ref={(el) => {
                sectionRefs.current.system = el;
              }}
            >
              <CardHeader className="pb-3">
                <CardTitle className="flex items-center gap-2 text-lg">
                  <Settings className="h-5 w-5 text-muted-foreground" />
                  Configuration système
                </CardTitle>
              </CardHeader>
              <CardContent className="space-y-6">
                {/* System Dependencies (Linux-specific) */}
                {currentPlatform === "linux" && (
                  <div className="space-y-3">
                    <Label>Permissions système</Label>
                    <p className="text-sm text-muted-foreground">
                      {systemDepsInstalled
                        ? "Permissions système configurées"
                        : "Configurer les permissions evdev/uinput"}
                    </p>

                    <div className="flex flex-col gap-2">
                      <div className="flex items-center gap-2">
                        <Badge variant={permissionsInstalled ? "default" : "secondary"}>
                          {permissionsInstalled ? "Actif" : "Manquant"}
                        </Badge>
                        <span className="text-sm">Appui pour parler (evdev)</span>
                      </div>
                      <div className="flex items-center gap-2">
                        <Badge variant={uinputAvailable ? "default" : "secondary"}>
                          {uinputAvailable ? "Actif" : "Manquant"}
                        </Badge>
                        <span className="text-sm">Saisie de texte (uinput)</span>
                      </div>
                    </div>

                    <Button
                      onClick={handleInstallSystem}
                      disabled={isInstallingSystem || systemDepsInstalled}
                      variant={systemDepsInstalled ? "outline" : "default"}
                      size="sm"
                    >
                      {isInstallingSystem
                        ? "Installation…"
                        : systemDepsInstalled
                          ? "Configuré"
                          : "Configurer les permissions"}
                    </Button>
                  </div>
                )}

                {currentPlatform === "linux" && <Separator />}

                {/* Paste Method */}
                <div className="space-y-3">
                  <Label htmlFor="paste-method">Méthode de collage</Label>
                  <p className="text-sm text-muted-foreground">
                    Comment le texte est injecté dans l'application active
                  </p>
                  <Select
                    value={pasteMethod}
                    onValueChange={(v) => setPasteMethod(v as import("@/bindings").PasteMethod)}
                  >
                    <SelectTrigger id="paste-method">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="Direct">
                        Direct (wtype/xdotool), recommandé sur Linux
                      </SelectItem>
                      <SelectItem value="CtrlV">Ctrl+V (clipboard)</SelectItem>
                      <SelectItem value="CtrlShiftV">Ctrl+Shift+V (terminaux)</SelectItem>
                      <SelectItem value="ShiftInsert">
                        Shift+Insert (tmux, certains terminaux)
                      </SelectItem>
                      <SelectItem value="None">Désactivé</SelectItem>
                    </SelectContent>
                  </Select>
                </div>

                <Separator />

                {/* Autostart */}
                <div className="space-y-3">
                  <Label htmlFor="autostart">Lancement au démarrage</Label>
                  <p className="text-sm text-muted-foreground">
                    Démarrer automatiquement KaraokeTyper à la connexion
                  </p>
                  <div className="flex items-center justify-between">
                    <span className="text-sm">
                      {autostart ? "Démarre à la connexion" : "Démarrage manuel"}
                    </span>
                    <Switch
                      id="autostart"
                      checked={autostart}
                      onCheckedChange={handleToggleAutostart}
                      disabled={isTogglingAutostart}
                    />
                  </div>
                </div>
              </CardContent>
            </Card>

            {/* GNOME Terminal Detection Extension (Linux/Wayland only) */}
            {isGnomeWayland && (
              <Card
                ref={(el) => {
                  sectionRefs.current.gnome = el;
                }}
              >
                <CardHeader className="pb-3">
                  <CardTitle className="flex items-center gap-2 text-lg">
                    <Monitor className="h-5 w-5 text-muted-foreground" />
                    Détection de terminal
                  </CardTitle>
                </CardHeader>
                <CardContent className="space-y-4">
                  <p className="text-sm text-muted-foreground">
                    Extension GNOME Shell pour la détection automatique des terminaux. Utilise
                    Ctrl+Maj+V dans les terminaux et Ctrl+V ailleurs.
                  </p>

                  <div className="flex items-center justify-between">
                    <div className="flex items-center gap-2">
                      <Badge
                        variant={gnomeExtInstalled && gnomeExtEnabled ? "default" : "secondary"}
                      >
                        {gnomeExtInstalled && gnomeExtEnabled
                          ? "Active"
                          : gnomeExtInstalled
                            ? "Installée (désactivée)"
                            : "Non installée"}
                      </Badge>
                      <span className="text-sm">
                        {gnomeExtInstalled && gnomeExtEnabled
                          ? "Détection automatique active"
                          : "Détection de secours utilisée"}
                      </span>
                    </div>

                    <div className="flex gap-2">
                      <Button
                        onClick={handleInstallGnomeExtension}
                        disabled={isInstallingGnomeExt || gnomeExtInstalled}
                        variant={gnomeExtInstalled ? "outline" : "default"}
                        size="sm"
                      >
                        {isInstallingGnomeExt
                          ? "Installation…"
                          : gnomeExtInstalled
                            ? "Installée"
                            : "Installer l'extension"}
                      </Button>
                      {gnomeExtInstalled && (
                        <Button
                          onClick={handleRemoveGnomeExtension}
                          disabled={isInstallingGnomeExt}
                          variant="destructive"
                          size="sm"
                        >
                          Supprimer
                        </Button>
                      )}
                    </div>
                  </div>

                  {gnomeExtInstalled && !gnomeExtEnabled && (
                    <p className="text-sm text-destructive">
                      Extension installée mais non activée. Activez-la dans l'application Extensions
                      GNOME ou lancez : gnome-extensions enable focused-window-dbus@karaoketyper
                    </p>
                  )}
                </CardContent>
              </Card>
            )}

            {/* Audio Feedback */}
            <Card
              ref={(el) => {
                sectionRefs.current.feedback = el;
              }}
            >
              <CardHeader className="pb-3">
                <CardTitle className="flex items-center gap-2 text-lg">
                  <Volume2 className="h-5 w-5 text-muted-foreground" />
                  Sons de l'application
                </CardTitle>
              </CardHeader>
              <CardContent className="space-y-4">
                <div className="flex items-center justify-between">
                  <div className="space-y-1">
                    <Label htmlFor="audio-feedback">Notifications sonores</Label>
                    <p className="text-sm text-muted-foreground">
                      Jouer un son au début et à la fin de l'enregistrement
                    </p>
                  </div>
                  <Switch
                    id="audio-feedback"
                    checked={audioFeedbackEnabled}
                    onCheckedChange={setAudioFeedbackEnabled}
                  />
                </div>

                {audioFeedbackEnabled && (
                  <div className="space-y-2">
                    <div className="flex items-center gap-2">
                      <button
                        type="button"
                        onClick={() => {
                          if (audioFeedbackVolume > 0) {
                            lastVolumeRef.current = audioFeedbackVolume;
                            setAudioFeedbackVolume(0);
                          } else {
                            setAudioFeedbackVolume(lastVolumeRef.current || 0.7);
                          }
                        }}
                        className="rounded-md text-muted-foreground hover:text-foreground transition-colors focus-visible:outline-none focus-visible:ring-[3px] focus-visible:ring-ring/50"
                        title={audioFeedbackVolume > 0 ? "Couper le son" : "Rétablir le son"}
                        aria-label={audioFeedbackVolume > 0 ? "Couper le son" : "Rétablir le son"}
                        aria-pressed={audioFeedbackVolume === 0}
                      >
                        {audioFeedbackVolume > 0 ? (
                          <Volume2 className="h-4 w-4" />
                        ) : (
                          <VolumeX className="h-4 w-4" />
                        )}
                      </button>
                      <Label id="audio-volume-label" className="flex-1">
                        Volume ({Math.round(audioFeedbackVolume * 100)}%)
                      </Label>
                    </div>
                    <Slider
                      aria-labelledby="audio-volume-label"
                      min={0}
                      max={1}
                      step={0.05}
                      value={[audioFeedbackVolume]}
                      onValueChange={([v]) => setAudioFeedbackVolume(v)}
                    />
                  </div>
                )}
              </CardContent>
            </Card>

            {/* Model Unload Timeout */}
            <Card
              ref={(el) => {
                sectionRefs.current.unload = el;
              }}
            >
              <CardHeader className="pb-3">
                <CardTitle className="flex items-center gap-2 text-lg">
                  <Clock className="h-5 w-5 text-muted-foreground" />
                  Déchargement du modèle
                </CardTitle>
              </CardHeader>
              <CardContent className="space-y-4">
                <div className="space-y-2">
                  <Label htmlFor="model-unload">Décharger après inactivité</Label>
                  <p className="text-sm text-muted-foreground">
                    Libérer la mémoire en déchargeant le modèle STT après une période d'inactivité.
                    Le modèle se recharge automatiquement.
                  </p>
                  <Select
                    value={modelUnloadTimeout}
                    onValueChange={(value) =>
                      setModelUnloadTimeout(value as "Never" | "Min5" | "Min15" | "Hour1")
                    }
                  >
                    <SelectTrigger id="model-unload">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="Never">Jamais</SelectItem>
                      <SelectItem value="Min5">Après 5 minutes</SelectItem>
                      <SelectItem value="Min15">Après 15 minutes</SelectItem>
                      <SelectItem value="Hour1">Après 1 heure</SelectItem>
                    </SelectContent>
                  </Select>
                </div>
              </CardContent>
            </Card>

            {/* Ollama Integration */}
            <Card
              ref={(el) => {
                sectionRefs.current.ollama = el;
              }}
            >
              <CardHeader className="pb-3">
                <CardTitle className="flex items-center gap-2 text-lg">
                  <SpellCheck className="h-5 w-5 text-muted-foreground" />
                  Intégration Ollama
                </CardTitle>
              </CardHeader>
              <CardContent className="space-y-4">
                <div className="flex items-center justify-between">
                  <div className="space-y-1">
                    <Label htmlFor="ollama-enabled">Correction de texte</Label>
                    <p className="text-sm text-muted-foreground">
                      Utiliser un LLM local pour corriger la grammaire et l'orthographe après la
                      transcription
                    </p>
                  </div>
                  <Switch
                    id="ollama-enabled"
                    checked={ollamaEnabled}
                    onCheckedChange={(enabled) => {
                      setOllamaEnabled(enabled);
                      if (enabled) void checkOllama();
                    }}
                  />
                </div>

                {ollamaEnabled && (
                  <>
                    <div className="space-y-2">
                      <Label>URL du serveur</Label>
                      <Input
                        value={ollamaUrl}
                        onChange={(e) => setOllamaUrl(e.target.value)}
                        placeholder="http://localhost:11434"
                      />
                    </div>

                    <div className="space-y-2">
                      <Label>Modèle</Label>
                      <Input
                        value={ollamaModel}
                        onChange={(e) => setOllamaModel(e.target.value)}
                        placeholder="mistral"
                      />
                    </div>

                    <div className="flex items-center justify-between">
                      <div className="flex items-center gap-2">
                        <Badge variant={ollamaStatus?.available ? "default" : "secondary"}>
                          {ollamaStatus?.available ? "Connecté" : "Déconnecté"}
                        </Badge>
                        <span className="text-sm text-muted-foreground">
                          {ollamaStatus?.available
                            ? `${ollamaStatus.model} prêt`
                            : "Ollama non joignable"}
                        </span>
                      </div>
                      <Button variant="outline" size="sm" onClick={() => void checkOllama()}>
                        <RefreshCw className="h-4 w-4 mr-1" />
                        Tester
                      </Button>
                    </div>
                  </>
                )}
              </CardContent>
            </Card>

            {/* About */}
            <Card
              ref={(el) => {
                sectionRefs.current.about = el;
              }}
            >
              <CardHeader className="pb-3">
                <CardTitle className="flex items-center gap-2 text-lg">
                  <Info className="h-5 w-5 text-muted-foreground" />À propos
                </CardTitle>
              </CardHeader>
              <CardContent className="flex flex-col gap-4">
                <div>
                  <p className="text-sm text-muted-foreground">
                    KaraokeTyper{appVersion ? ` v${appVersion}` : ""}
                  </p>
                  <p className="text-sm text-muted-foreground">
                    Dictée vocale hors-ligne pour{" "}
                    {currentPlatform === "macos"
                      ? "macOS"
                      : currentPlatform === "windows"
                        ? "Windows"
                        : "Linux"}
                  </p>
                </div>
                <UpdateChecker />
              </CardContent>
            </Card>
          </div>
        </ScrollArea>
      </div>

      {/* Fixed Footer */}
      <div className="flex-none flex items-center justify-end gap-4 px-6 py-4 border-t border-border">
        {saveStatus && (
          <output
            aria-live="polite"
            className={cn(
              "text-sm",
              saveStatus.startsWith("Erreur") ? "text-destructive" : "text-muted-foreground",
            )}
          >
            {saveStatus}
          </output>
        )}
        <Button variant="ghost" onClick={handleCancel} disabled={isSaving}>
          Annuler
        </Button>
        <Button onClick={handleSave} disabled={isSaving}>
          Enregistrer et fermer
        </Button>
      </div>
    </div>
  );
}

export default SettingsWindow;
