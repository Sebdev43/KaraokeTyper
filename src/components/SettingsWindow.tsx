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
  Sparkles,
  Volume2,
  VolumeX,
  X,
} from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { type AppConfig, commands } from "@/bindings";
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

// Event type for model download progress (not in generated bindings)
interface DownloadProgress {
  model: string;
  downloaded_mb: number;
  total_mb: number;
  percent: number;
  status: string;
}

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
  const [activeSection, setActiveSection] = useState("audio");
  const inputRef = useRef<HTMLInputElement>(null);
  const sectionRefs = useRef<Record<string, HTMLDivElement | null>>({});
  const lastVolumeRef = useRef(0.7);

  // Detect platform on mount
  useEffect(() => {
    try {
      const p = platform();
      setPlatform(p as "linux" | "windows" | "macos" | "unknown");
    } catch {
      setPlatform("linux");
    }
  }, [setPlatform]);

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
        setSelectedDevice(result.data.audio_device_index);
        setSttBackend(result.data.stt_backend ?? "whisper");
        setModel(result.data.model);
        setLanguage(result.data.language);
        setUseGpuAcceleration(result.data.use_gpu ?? false);
        setAudioFeedbackEnabled(result.data.audio_feedback_enabled ?? true);
        setAudioFeedbackVolume(result.data.audio_feedback_volume ?? 0.7);
        setModelUnloadTimeout(result.data.model_unload_timeout ?? "Never");
        setOllamaEnabled(result.data.ollama_enabled ?? false);
        setOllamaUrl(result.data.ollama_url ?? "http://localhost:11434");
        setOllamaModel(result.data.ollama_model ?? "mistral");
        setPasteMethod(result.data.paste_method ?? "Direct");
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
    setUseGpuAcceleration,
    setAudioFeedbackEnabled,
    setAudioFeedbackVolume,
    setModelUnloadTimeout,
    setOllamaEnabled,
    setOllamaUrl,
    setOllamaModel,
    setPasteMethod,
  ]);

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
    checkGnomeExtension,
    loadAutostart,
    loadGpuInfo,
    loadGpuStatus,
  ]);

  useEffect(() => {
    const unlistenProgress = listen<DownloadProgress>(
      "model-download-progress",
      (event) => {
        setDownloadProgress(event.payload.percent);
      },
    );

    const unlistenComplete = listen("model-download-complete", () => {
      // Refresh models list when download completes
      void loadInstalledModels();
    });

    return () => {
      void unlistenProgress.then((fn) => fn());
      void unlistenComplete.then((fn) => fn());
    };
  }, [loadInstalledModels, setDownloadProgress]);

  const handleShortcutCapture = (e: React.MouseEvent) => {
    e.preventDefault();
    setIsCapturing(true);
    setShortcut("Press any key...");
    inputRef.current?.focus();
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (!isCapturing) return;

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

    if (key === "Control" || key === "Alt" || key === "Shift" || key === "Meta")
      return;

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
    if (isCapturing) {
      setShortcut("Ctrl+Space");
      setIsCapturing(false);
    }
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
    try {
      setSaveStatus("Enregistrement…");

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
        key_delay_ms: 8,
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
      };

      const result = await commands.saveSettings(config);
      if (result.status === "ok") {
        setSaveStatus("Paramètres enregistrés !");
        setTimeout(() => setSaveStatus(""), 2000);
        await getCurrentWindow().hide();
      } else {
        setSaveStatus(`Error: ${result.error}`);
        console.error("Failed to save settings:", result.error);
      }
    } catch (error) {
      setSaveStatus(`Error: ${error}`);
      console.error("Failed to save settings:", error);
    }
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
    const confirmed = await confirm(
      "Supprimer l'extension de détection de terminal ?",
      {
        title: "Supprimer Extension",
        kind: "warning",
      },
    );

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
    await getCurrentWindow().hide();
  };

  return (
    <div className="flex flex-col h-screen overflow-hidden bg-background">
      {/* Custom Titlebar - needed for Wayland compatibility */}
      <div
        data-tauri-drag-region
        className="flex-none flex items-center justify-between h-10 px-3 border-b border-border bg-muted/50 select-none"
      >
        <div data-tauri-drag-region className="flex items-center gap-2">
          <Settings className="h-4 w-4 text-primary" />
          <span className="text-sm font-medium">KaraokeTyper — Paramètres</span>
        </div>
        <div className="flex items-center gap-1">
          <Button
            variant="ghost"
            size="icon"
            className="h-7 w-7 hover:bg-muted"
            onClick={handleMinimize}
          >
            <Minimize2 className="h-3.5 w-3.5" />
          </Button>
          <Button
            variant="ghost"
            size="icon"
            className="h-7 w-7 hover:bg-muted"
            onClick={handleMaximize}
          >
            <Maximize2 className="h-3.5 w-3.5" />
          </Button>
          <Button
            variant="ghost"
            size="icon"
            className="h-7 w-7 hover:bg-destructive hover:text-destructive-foreground"
            onClick={handleClose}
          >
            <X className="h-3.5 w-3.5" />
          </Button>
        </div>
      </div>

      {/* Sidebar + Content */}
      <div className="flex flex-1 min-h-0">
        {/* Sidebar Navigation */}
        <nav className="flex-none w-48 border-r border-border bg-muted/30 py-2 overflow-y-auto">
          {[
            { id: "audio", icon: Mic, label: "Audio", color: "text-rose-400" },
            {
              id: "shortcut",
              icon: Keyboard,
              label: "Raccourci",
              color: "text-amber-400",
            },
            {
              id: "stt",
              icon: Brain,
              label: "Reconnaissance",
              color: "text-violet-400",
            },
            {
              id: "performance",
              icon: Cpu,
              label: "Performance",
              color: "text-cyan-400",
            },
            {
              id: "models",
              icon: Download,
              label: "Modèles",
              color: "text-emerald-400",
            },
            {
              id: "system",
              icon: Settings,
              label: "Système",
              color: "text-orange-400",
            },
            ...(isGnomeWayland
              ? [
                  {
                    id: "gnome",
                    icon: Monitor,
                    label: "Terminal",
                    color: "text-sky-400",
                  },
                ]
              : []),
            {
              id: "feedback",
              icon: Volume2,
              label: "Sons",
              color: "text-pink-400",
            },
            {
              id: "unload",
              icon: Clock,
              label: "Déchargement",
              color: "text-teal-400",
            },
            {
              id: "ollama",
              icon: Sparkles,
              label: "Ollama",
              color: "text-fuchsia-400",
            },
            {
              id: "about",
              icon: Info,
              label: "À propos",
              color: "text-zinc-400",
            },
          ].map(({ id, icon: Icon, label, color }) => (
            <button
              key={id}
              type="button"
              onClick={() => {
                setActiveSection(id);
                sectionRefs.current[id]?.scrollIntoView({
                  behavior: "smooth",
                  block: "start",
                });
              }}
              className={cn(
                "flex items-center gap-2.5 w-full px-4 py-2 text-sm transition-colors text-left",
                activeSection === id
                  ? "bg-primary/10 text-foreground font-medium border-r-2 border-primary"
                  : "text-muted-foreground hover:text-foreground hover:bg-muted/50",
              )}
            >
              <Icon
                className={cn(
                  "h-4 w-4 shrink-0",
                  activeSection === id ? color : "text-muted-foreground",
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
                  <Mic className="h-5 w-5 text-rose-400" />
                  Entrée audio
                </CardTitle>
              </CardHeader>
              <CardContent className="space-y-4">
                <div className="space-y-2">
                  <Label htmlFor="microphone">Microphone</Label>
                  <div className="flex gap-2">
                    <Select
                      value={String(selectedDevice)}
                      onValueChange={(value) =>
                        setSelectedDevice(Number(value))
                      }
                    >
                      <SelectTrigger className="flex-1">
                        <SelectValue placeholder="Sélectionner un micro" />
                      </SelectTrigger>
                      <SelectContent>
                        {audioDevices.map((device) => (
                          <SelectItem
                            key={device.index}
                            value={String(device.index)}
                          >
                            {device.name}
                          </SelectItem>
                        ))}
                      </SelectContent>
                    </Select>
                    <Button
                      variant="outline"
                      size="icon"
                      onClick={handleRefreshDevices}
                      disabled={isRefreshing}
                    >
                      <RefreshCw
                        className={cn(
                          "h-4 w-4",
                          isRefreshing && "animate-spin",
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
                  <Keyboard className="h-5 w-5 text-amber-400" />
                  Raccourci clavier
                </CardTitle>
              </CardHeader>
              <CardContent className="space-y-4">
                <div className="space-y-2">
                  <Label htmlFor="shortcut">Touche d'enregistrement</Label>
                  <Input
                    ref={inputRef}
                    id="shortcut"
                    type="text"
                    value={shortcut}
                    onClick={handleShortcutCapture}
                    onKeyDown={handleKeyDown}
                    onMouseDown={handleMouseDown}
                    onBlur={handleBlur}
                    onContextMenu={(e) => e.preventDefault()}
                    readOnly
                    placeholder="Cliquez pour définir..."
                    className={cn(
                      "cursor-pointer",
                      isCapturing && "ring-2 ring-primary bg-primary/10",
                    )}
                  />
                  <p className="text-xs text-muted-foreground">
                    Cliquez, puis appuyez sur une touche ou un bouton souris
                    (Mouse4, Mouse5...)
                  </p>
                </div>
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
                  <Brain className="h-5 w-5 text-violet-400" />
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
                        (m) =>
                          (m.engine_type ?? "whisper") === next && m.installed,
                      );
                      if (first) setModel(first.name);
                    }}
                  >
                    <SelectTrigger id="stt-backend">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="whisper">
                        Whisper — généraliste, multi-langue (FR inclus)
                      </SelectItem>
                      <SelectItem value="sense-voice" disabled>
                        SenseVoice — multilingue (FR/EN/ES/DE) · bientôt
                      </SelectItem>
                      <SelectItem value="moonshine" disabled>
                        Moonshine — ultra-léger (EN) · bientôt
                      </SelectItem>
                      <SelectItem value="voxtral">
                        Voxtral Mini — auto-hébergé (FR WER ≈ 4.9 %)
                      </SelectItem>
                    </SelectContent>
                  </Select>
                  <p className="text-xs text-muted-foreground">
                    100 % local. Les moteurs « bientôt » arrivent quand la
                    feature ONNX de transcribe-rs sera activée.
                  </p>
                </div>

                <div className="space-y-2">
                  <Label htmlFor="model">Modèle actif</Label>
                  <Select value={model} onValueChange={setModel}>
                    <SelectTrigger>
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      {installedModels
                        .filter(
                          (m) => (m.engine_type ?? "whisper") === sttBackend,
                        )
                        .map((modelInfo) => (
                          <SelectItem
                            key={modelInfo.name}
                            value={modelInfo.name}
                            disabled={
                              !modelInfo.installed || modelInfo.coming_soon
                            }
                          >
                            {modelInfo.display_name} - {modelInfo.size_mb} MB
                            {modelInfo.coming_soon && " (bientôt)"}
                            {!modelInfo.installed &&
                              !modelInfo.coming_soon &&
                              " (Non installé)"}
                          </SelectItem>
                        ))}
                    </SelectContent>
                  </Select>
                  {model &&
                    !installedModels.find((m) => m.name === model)
                      ?.installed && (
                      <p className="text-xs text-destructive">
                        ⚠️ Le modèle "{model}" n'est pas installé.
                        Téléchargez-le dans "Modèles" ci-dessous.
                      </p>
                    )}
                </div>

                <div className="space-y-2">
                  <Label htmlFor="language">Langue</Label>
                  <Select value={language} onValueChange={setLanguage}>
                    <SelectTrigger>
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
                  <Cpu className="h-5 w-5 text-cyan-400" />
                  Performance
                </CardTitle>
              </CardHeader>
              <CardContent className="space-y-4">
                {/* GPU Info */}
                <div className="flex items-center justify-between">
                  <div className="space-y-1">
                    <Label>GPU détecté</Label>
                    <p className="text-sm text-muted-foreground">
                      {gpuInfo
                        ? gpuInfo.device_name || "Aucun GPU détecté"
                        : "Détection..."}
                      {gpuInfo?.vram_mb
                        ? ` (${gpuInfo.vram_mb >= 1024 ? `${(gpuInfo.vram_mb / 1024).toFixed(1)} GB` : `${gpuInfo.vram_mb} MB`} VRAM)`
                        : ""}
                    </p>
                  </div>
                  <Badge variant={gpuInfo?.available ? "default" : "secondary"}>
                    {gpuInfo
                      ? gpuInfo.available
                        ? gpuInfo.backend
                        : "CPU seul"
                      : "..."}
                  </Badge>
                </div>

                {/* VRAM Recommendation */}
                {gpuInfo?.vram_mb && gpuInfo.recommended_model && (
                  <div className="p-2 rounded bg-cyan-500/10 border border-cyan-500/30">
                    <p className="text-sm text-cyan-400">
                      Modèle max recommandé pour votre GPU :{" "}
                      <span className="font-medium">
                        {gpuInfo.recommended_model}
                      </span>
                    </p>
                    {(() => {
                      const vramTable: Record<string, number> = {
                        "ggml-tiny": 150,
                        "ggml-base": 300,
                        "ggml-small": 700,
                        "ggml-medium": 1500,
                        "ggml-large-v3": 3000,
                      };
                      const currentVram = vramTable[model] ?? 0;
                      const isTooLarge =
                        gpuInfo.vram_mb !== null &&
                        currentVram > gpuInfo.vram_mb * 0.7;
                      if (useGpuAcceleration && isTooLarge) {
                        return (
                          <p className="text-xs text-yellow-400 mt-1">
                            Votre modèle actuel ({model}) risque de dépasser la
                            capacité VRAM et provoquer des crashs. Essayez{" "}
                            {gpuInfo.recommended_model}.
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
                    <Badge
                      variant={
                        gpuStatus.mode === "gpu" ? "default" : "secondary"
                      }
                      className={cn(
                        gpuStatus.mode === "gpu" && "bg-green-600",
                        gpuStatus.mode === "cpu" &&
                          gpuStatus.fallback_reason &&
                          "bg-yellow-600",
                      )}
                    >
                      {gpuStatus.mode === "gpu" ? "GPU actif" : "Mode CPU"}
                    </Badge>
                  </div>
                )}

                {/* GPU Failure Warning */}
                {gpuInfo && gpuInfo.failure_count > 0 && (
                  <div className="flex items-center justify-between p-2 rounded bg-yellow-500/10 border border-yellow-500/30">
                    <div className="space-y-1">
                      <p className="text-sm text-yellow-500 font-medium">
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
                    <Label>Accélération GPU</Label>
                    <p className="text-sm text-muted-foreground">
                      {useGpuAcceleration
                        ? "GPU utilisé pour la transcription"
                        : "CPU utilisé (plus compatible)"}
                    </p>
                  </div>
                  <Switch
                    checked={useGpuAcceleration}
                    onCheckedChange={setUseGpuAcceleration}
                    disabled={
                      !gpuInfo?.available || gpuInfo?.disabled_by_failures
                    }
                  />
                </div>

                {!gpuInfo?.available && (
                  <p className="text-xs text-muted-foreground">
                    Accélération GPU non disponible. Compilé sans support GPU ou
                    aucun GPU compatible détecté.
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
                  <Download className="h-5 w-5 text-emerald-400" />
                  Gestion des modèles
                </CardTitle>
              </CardHeader>
              <CardContent className="space-y-3">
                <p className="text-sm text-muted-foreground">
                  Télécharger les modèles Whisper GGML pour la reconnaissance
                  vocale
                </p>

                {installedModels.map((modelInfo) => (
                  <div
                    key={modelInfo.name}
                    className="flex items-center justify-between p-3 rounded-lg border border-border"
                  >
                    <div className="flex-1">
                      <div className="flex items-center gap-2">
                        <span className="font-medium">
                          {modelInfo.display_name}
                        </span>
                        {modelInfo.installed && (
                          <Badge variant="default" className="text-xs">
                            Installé
                          </Badge>
                        )}
                        {modelInfo.name === "ggml-small" && (
                          <Badge variant="secondary" className="text-xs">
                            Recommandé
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

                    <Button
                      variant={modelInfo.installed ? "outline" : "default"}
                      size="sm"
                      onClick={() => handleDownloadModel(modelInfo.name)}
                      disabled={
                        modelInfo.installed || downloadingModel !== null
                      }
                    >
                      {modelInfo.installed ? "Installé ✓" : "Télécharger"}
                    </Button>
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
                  <Settings className="h-5 w-5 text-orange-400" />
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
                        <Badge
                          variant={
                            permissionsInstalled ? "default" : "secondary"
                          }
                        >
                          {permissionsInstalled ? "✓" : "○"}
                        </Badge>
                        <span className="text-sm">
                          Appui pour parler (evdev)
                        </span>
                      </div>
                      <div className="flex items-center gap-2">
                        <Badge
                          variant={uinputAvailable ? "default" : "secondary"}
                        >
                          {uinputAvailable ? "✓" : "○"}
                        </Badge>
                        <span className="text-sm">
                          Saisie de texte (uinput)
                        </span>
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
                          ? "Configuré ✓"
                          : "Configurer les permissions"}
                    </Button>
                  </div>
                )}

                {currentPlatform === "linux" && <Separator />}

                {/* Paste Method */}
                <div className="space-y-3">
                  <Label>Méthode de collage</Label>
                  <p className="text-sm text-muted-foreground">
                    Comment le texte est injecté dans l'application active
                  </p>
                  <Select
                    value={pasteMethod}
                    onValueChange={(v) =>
                      setPasteMethod(v as import("@/bindings").PasteMethod)
                    }
                  >
                    <SelectTrigger>
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="Direct">
                        Direct (wtype/xdotool) — recommandé Linux
                      </SelectItem>
                      <SelectItem value="CtrlV">Ctrl+V (clipboard)</SelectItem>
                      <SelectItem value="CtrlShiftV">
                        Ctrl+Shift+V (terminaux)
                      </SelectItem>
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
                  <Label>Lancement au démarrage</Label>
                  <p className="text-sm text-muted-foreground">
                    Démarrer automatiquement KaraokeTyper à la connexion
                  </p>
                  <div className="flex items-center justify-between">
                    <div className="flex items-center gap-2">
                      <Badge variant={autostart ? "default" : "secondary"}>
                        {autostart ? "Activé" : "Désactivé"}
                      </Badge>
                      <span className="text-sm">
                        {autostart
                          ? "Démarre à la connexion"
                          : "Démarrage manuel"}
                      </span>
                    </div>
                    <Switch
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
                    <Monitor className="h-5 w-5 text-sky-400" />
                    Détection de terminal
                  </CardTitle>
                </CardHeader>
                <CardContent className="space-y-4">
                  <p className="text-sm text-muted-foreground">
                    Extension GNOME Shell pour la détection automatique des
                    terminaux. Utilise Ctrl+Maj+V dans les terminaux et Ctrl+V
                    ailleurs.
                  </p>

                  <div className="flex items-center justify-between">
                    <div className="flex items-center gap-2">
                      <Badge
                        variant={
                          gnomeExtInstalled && gnomeExtEnabled
                            ? "default"
                            : "secondary"
                        }
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
                            ? "Installée ✓"
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
                    <p className="text-sm text-yellow-500">
                      Extension installée mais non activée. Activez-la dans
                      l'application Extensions GNOME ou lancez :
                      gnome-extensions enable focused-window-dbus@karaoketyper
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
                  <Volume2 className="h-5 w-5 text-pink-400" />
                  Sons de l'application
                </CardTitle>
              </CardHeader>
              <CardContent className="space-y-4">
                <div className="flex items-center justify-between">
                  <div className="space-y-1">
                    <Label>Notifications sonores</Label>
                    <p className="text-sm text-muted-foreground">
                      Jouer un son au début et à la fin de l'enregistrement
                    </p>
                  </div>
                  <Switch
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
                            setAudioFeedbackVolume(
                              lastVolumeRef.current || 0.7,
                            );
                          }
                        }}
                        className="text-muted-foreground hover:text-foreground transition-colors"
                        title={
                          audioFeedbackVolume > 0
                            ? "Couper le son"
                            : "Rétablir le son"
                        }
                      >
                        {audioFeedbackVolume > 0 ? (
                          <Volume2 className="h-4 w-4" />
                        ) : (
                          <VolumeX className="h-4 w-4" />
                        )}
                      </button>
                      <Label className="flex-1">
                        Volume ({Math.round(audioFeedbackVolume * 100)}%)
                      </Label>
                    </div>
                    <Slider
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
                  <Clock className="h-5 w-5 text-teal-400" />
                  Déchargement du modèle
                </CardTitle>
              </CardHeader>
              <CardContent className="space-y-4">
                <div className="space-y-2">
                  <Label>Décharger après inactivité</Label>
                  <p className="text-sm text-muted-foreground">
                    Libérer la mémoire en déchargeant le modèle STT après une
                    période d'inactivité. Le modèle se recharge automatiquement.
                  </p>
                  <Select
                    value={modelUnloadTimeout}
                    onValueChange={(value) =>
                      setModelUnloadTimeout(
                        value as "Never" | "Min5" | "Min15" | "Hour1",
                      )
                    }
                  >
                    <SelectTrigger>
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
                  <Sparkles className="h-5 w-5 text-fuchsia-400" />
                  Intégration Ollama
                </CardTitle>
              </CardHeader>
              <CardContent className="space-y-4">
                <div className="flex items-center justify-between">
                  <div className="space-y-1">
                    <Label>Correction de texte</Label>
                    <p className="text-sm text-muted-foreground">
                      Utiliser un LLM local pour corriger la grammaire et
                      l'orthographe après la transcription
                    </p>
                  </div>
                  <Switch
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
                        <Badge
                          variant={
                            ollamaStatus?.available ? "default" : "secondary"
                          }
                          className={cn(
                            ollamaStatus?.available && "bg-green-600",
                          )}
                        >
                          {ollamaStatus?.available ? "Connecté" : "Déconnecté"}
                        </Badge>
                        <span className="text-sm text-muted-foreground">
                          {ollamaStatus?.available
                            ? `${ollamaStatus.model} prêt`
                            : "Ollama non joignable"}
                        </span>
                      </div>
                      <Button
                        variant="outline"
                        size="sm"
                        onClick={() => void checkOllama()}
                      >
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
                  <Info className="h-5 w-5 text-zinc-400" />À propos
                </CardTitle>
              </CardHeader>
              <CardContent>
                <p className="text-sm text-muted-foreground">
                  KaraokeTyper v1.1.0
                </p>
                <p className="text-sm text-muted-foreground">
                  Dictée vocale hors-ligne pour{" "}
                  {currentPlatform === "macos"
                    ? "macOS"
                    : currentPlatform === "windows"
                      ? "Windows"
                      : "Linux"}
                </p>
              </CardContent>
            </Card>
          </div>
        </ScrollArea>
      </div>

      {/* Fixed Footer */}
      <div className="flex-none flex items-center justify-end gap-4 px-6 py-4 border-t border-border">
        {saveStatus && (
          <span
            className={cn(
              "text-sm",
              saveStatus.startsWith("Error")
                ? "text-destructive"
                : "text-green-500",
            )}
          >
            {saveStatus}
          </span>
        )}
        <Button onClick={handleSave}>Enregistrer et fermer</Button>
      </div>
    </div>
  );
}

export default SettingsWindow;
