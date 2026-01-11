import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { confirm, message } from "@tauri-apps/plugin-dialog";
import { platform } from "@tauri-apps/plugin-os";
import { relaunch } from "@tauri-apps/plugin-process";
import {
  Brain,
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
  const inputRef = useRef<HTMLInputElement>(null);

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
        setModel(result.data.model);
        setLanguage(result.data.language);
        setUseGpuAcceleration(result.data.use_gpu ?? false);
      } else {
        console.error("Failed to load settings:", result.error);
      }
    } catch (error) {
      console.error("Failed to load settings:", error);
    }
  }, [setShortcut, setSelectedDevice, setModel, setLanguage, setUseGpuAcceleration]);

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
      setSaveStatus("Saving...");

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
      };

      const result = await commands.saveSettings(config);
      if (result.status === "ok") {
        setSaveStatus("Settings saved!");
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
      "Configurer les permissions système ?\n\nCela va :\n• Configurer evdev pour le push-to-talk\n• Peut nécessiter le redémarrage de l'application",
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
          <span className="text-sm font-medium">KaraokeTyper Settings</span>
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

      {/* Content */}
      <ScrollArea className="flex-1 min-h-0">
        <div className="p-6 space-y-6">
          {/* Audio Input */}
          <Card>
            <CardHeader className="pb-3">
              <CardTitle className="flex items-center gap-2 text-lg">
                <Mic className="h-5 w-5 text-primary" />
                Audio Input
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
                    <SelectTrigger className="flex-1">
                      <SelectValue placeholder="Select a microphone" />
                    </SelectTrigger>
                    <SelectContent>
                      {audioDevices.map((device) => (
                        <SelectItem key={device.index} value={String(device.index)}>
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
                    <RefreshCw className={cn("h-4 w-4", isRefreshing && "animate-spin")} />
                  </Button>
                </div>
              </div>
            </CardContent>
          </Card>

          {/* Keyboard Shortcut */}
          <Card>
            <CardHeader className="pb-3">
              <CardTitle className="flex items-center gap-2 text-lg">
                <Keyboard className="h-5 w-5 text-primary" />
                Keyboard Shortcut
              </CardTitle>
            </CardHeader>
            <CardContent className="space-y-4">
              <div className="space-y-2">
                <Label htmlFor="shortcut">Recording Hotkey</Label>
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
                  placeholder="Click to set hotkey..."
                  className={cn(
                    "cursor-pointer",
                    isCapturing && "ring-2 ring-primary bg-primary/10",
                  )}
                />
                <p className="text-xs text-muted-foreground">
                  Click, then press any key or mouse button (Mouse4, Mouse5...)
                </p>
              </div>
            </CardContent>
          </Card>

          {/* Speech Recognition */}
          <Card>
            <CardHeader className="pb-3">
              <CardTitle className="flex items-center gap-2 text-lg">
                <Brain className="h-5 w-5 text-primary" />
                Speech Recognition
              </CardTitle>
            </CardHeader>
            <CardContent className="space-y-4">
              <div className="space-y-2">
                <Label htmlFor="model">Active Model</Label>
                <Select value={model} onValueChange={setModel}>
                  <SelectTrigger>
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    {installedModels.map((modelInfo) => (
                      <SelectItem
                        key={modelInfo.name}
                        value={modelInfo.name}
                        disabled={!modelInfo.installed}
                      >
                        {modelInfo.display_name} - {modelInfo.size_mb} MB
                        {!modelInfo.installed && " (Non installé)"}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
                {model && !installedModels.find((m) => m.name === model)?.installed && (
                  <p className="text-xs text-destructive">
                    ⚠️ Le modèle "{model}" n'est pas installé. Téléchargez-le dans "Model Management"
                    ci-dessous.
                  </p>
                )}
              </div>

              <div className="space-y-2">
                <Label htmlFor="language">Language</Label>
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
          <Card>
            <CardHeader className="pb-3">
              <CardTitle className="flex items-center gap-2 text-lg">
                <Cpu className="h-5 w-5 text-primary" />
                Performance
              </CardTitle>
            </CardHeader>
            <CardContent className="space-y-4">
              {/* GPU Info */}
              <div className="flex items-center justify-between">
                <div className="space-y-1">
                  <Label>Detected GPU</Label>
                  <p className="text-sm text-muted-foreground">
                    {gpuInfo ? gpuInfo.device_name || "No GPU detected" : "Detecting..."}
                  </p>
                </div>
                <Badge variant={gpuInfo?.available ? "default" : "secondary"}>
                  {gpuInfo ? (gpuInfo.available ? gpuInfo.backend : "CPU Only") : "..."}
                </Badge>
              </div>

              {/* Current Mode Status */}
              {gpuStatus && (
                <div className="flex items-center justify-between">
                  <div className="space-y-1">
                    <Label>Current Mode</Label>
                    <p className="text-sm text-muted-foreground">
                      {gpuStatus.fallback_reason || "Running normally"}
                    </p>
                  </div>
                  <Badge
                    variant={gpuStatus.mode === "gpu" ? "default" : "secondary"}
                    className={cn(
                      gpuStatus.mode === "gpu" && "bg-green-600",
                      gpuStatus.mode === "cpu" && gpuStatus.fallback_reason && "bg-yellow-600",
                    )}
                  >
                    {gpuStatus.mode === "gpu" ? "GPU Active" : "CPU Mode"}
                  </Badge>
                </div>
              )}

              {/* GPU Failure Warning */}
              {gpuInfo && gpuInfo.failure_count > 0 && (
                <div className="flex items-center justify-between p-2 rounded bg-yellow-500/10 border border-yellow-500/30">
                  <div className="space-y-1">
                    <p className="text-sm text-yellow-500 font-medium">
                      {gpuInfo.disabled_by_failures
                        ? `GPU auto-disabled after ${gpuInfo.failure_count} failures`
                        : `${gpuInfo.failure_count} GPU failure(s) recorded`}
                    </p>
                    <p className="text-xs text-muted-foreground">
                      {gpuInfo.disabled_by_failures
                        ? "GPU will not be used until you reset the counter"
                        : "GPU will be auto-disabled after 3 consecutive failures"}
                    </p>
                  </div>
                  <Button
                    size="sm"
                    variant="outline"
                    onClick={handleResetGpuFailures}
                    className="shrink-0"
                  >
                    <RefreshCw className="h-4 w-4 mr-1" />
                    Retry GPU
                  </Button>
                </div>
              )}

              <Separator />

              {/* GPU Toggle */}
              <div className="flex items-center justify-between">
                <div className="space-y-1">
                  <Label>GPU Acceleration</Label>
                  <p className="text-sm text-muted-foreground">
                    {useGpuAcceleration
                      ? "Using GPU for faster transcription"
                      : "Using CPU (safer, more compatible)"}
                  </p>
                </div>
                <Switch
                  checked={useGpuAcceleration}
                  onCheckedChange={setUseGpuAcceleration}
                  disabled={!gpuInfo?.available || gpuInfo?.disabled_by_failures}
                />
              </div>

              {/* Warning */}
              {useGpuAcceleration && !gpuInfo?.disabled_by_failures && (
                <p className="text-xs text-yellow-500">
                  GPU acceleration requires app restart. If you experience crashes, disable this
                  option.
                </p>
              )}
              {!gpuInfo?.available && (
                <p className="text-xs text-muted-foreground">
                  GPU acceleration not available. App compiled without GPU support or no compatible
                  GPU found.
                </p>
              )}
            </CardContent>
          </Card>

          {/* Model Management */}
          <Card>
            <CardHeader className="pb-3">
              <CardTitle className="flex items-center gap-2 text-lg">
                <Download className="h-5 w-5 text-primary" />
                Model Management
              </CardTitle>
            </CardHeader>
            <CardContent className="space-y-3">
              <p className="text-sm text-muted-foreground">
                Download Whisper GGML models for speech recognition
              </p>

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
                          Installed
                        </Badge>
                      )}
                      {modelInfo.name === "ggml-small" && (
                        <Badge variant="secondary" className="text-xs">
                          Recommended
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
                          Downloading... {downloadProgress.toFixed(1)}%
                        </p>
                      </div>
                    )}
                  </div>

                  <Button
                    variant={modelInfo.installed ? "outline" : "default"}
                    size="sm"
                    onClick={() => handleDownloadModel(modelInfo.name)}
                    disabled={modelInfo.installed || downloadingModel !== null}
                  >
                    {modelInfo.installed ? "Installed ✓" : "Download"}
                  </Button>
                </div>
              ))}
            </CardContent>
          </Card>

          {/* System Setup */}
          <Card>
            <CardHeader className="pb-3">
              <CardTitle className="flex items-center gap-2 text-lg">
                <Settings className="h-5 w-5 text-primary" />
                System Setup
              </CardTitle>
            </CardHeader>
            <CardContent className="space-y-6">
              {/* System Dependencies (Linux-specific) */}
              {currentPlatform === "linux" && (
                <div className="space-y-3">
                  <Label>System Permissions</Label>
                  <p className="text-sm text-muted-foreground">
                    {systemDepsInstalled
                      ? "System permissions configured"
                      : "Configure evdev/uinput permissions"}
                  </p>

                  <div className="flex flex-col gap-2">
                    <div className="flex items-center gap-2">
                      <Badge variant={permissionsInstalled ? "default" : "secondary"}>
                        {permissionsInstalled ? "✓" : "○"}
                      </Badge>
                      <span className="text-sm">Push-to-talk (evdev)</span>
                    </div>
                    <div className="flex items-center gap-2">
                      <Badge variant={uinputAvailable ? "default" : "secondary"}>
                        {uinputAvailable ? "✓" : "○"}
                      </Badge>
                      <span className="text-sm">Text output (uinput)</span>
                    </div>
                  </div>

                  <Button
                    onClick={handleInstallSystem}
                    disabled={isInstallingSystem || systemDepsInstalled}
                    variant={systemDepsInstalled ? "outline" : "default"}
                    size="sm"
                  >
                    {isInstallingSystem
                      ? "Installing..."
                      : systemDepsInstalled
                        ? "Configured ✓"
                        : "Setup Permissions"}
                  </Button>
                </div>
              )}

              {currentPlatform === "linux" && <Separator />}

              {/* Autostart */}
              <div className="space-y-3">
                <Label>Launch at Startup</Label>
                <p className="text-sm text-muted-foreground">
                  Automatically start KaraokeTyper when you log in
                </p>
                <div className="flex items-center justify-between">
                  <div className="flex items-center gap-2">
                    <Badge variant={autostart ? "default" : "secondary"}>
                      {autostart ? "Enabled" : "Disabled"}
                    </Badge>
                    <span className="text-sm">
                      {autostart ? "Starts at login" : "Manual start only"}
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
            <Card>
              <CardHeader className="pb-3">
                <CardTitle className="flex items-center gap-2 text-lg">
                  <Monitor className="h-5 w-5 text-primary" />
                  Terminal Detection
                </CardTitle>
              </CardHeader>
              <CardContent className="space-y-4">
                <p className="text-sm text-muted-foreground">
                  GNOME Shell extension for automatic terminal detection. When active, uses
                  Ctrl+Shift+V in terminals and Ctrl+V elsewhere.
                </p>

                <div className="flex items-center justify-between">
                  <div className="flex items-center gap-2">
                    <Badge variant={gnomeExtInstalled && gnomeExtEnabled ? "default" : "secondary"}>
                      {gnomeExtInstalled && gnomeExtEnabled
                        ? "Active"
                        : gnomeExtInstalled
                          ? "Installed (disabled)"
                          : "Not installed"}
                    </Badge>
                    <span className="text-sm">
                      {gnomeExtInstalled && gnomeExtEnabled
                        ? "Auto-detect active"
                        : "Using fallback detection"}
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
                        ? "Installing..."
                        : gnomeExtInstalled
                          ? "Installed ✓"
                          : "Install Extension"}
                    </Button>
                    {gnomeExtInstalled && (
                      <Button
                        onClick={handleRemoveGnomeExtension}
                        disabled={isInstallingGnomeExt}
                        variant="destructive"
                        size="sm"
                      >
                        Remove
                      </Button>
                    )}
                  </div>
                </div>

                {gnomeExtInstalled && !gnomeExtEnabled && (
                  <p className="text-sm text-yellow-500">
                    Extension installed but not enabled. Enable it in GNOME Extensions app or run:
                    gnome-extensions enable focused-window-dbus@karaoketyper
                  </p>
                )}
              </CardContent>
            </Card>
          )}

          {/* About */}
          <Card>
            <CardHeader className="pb-3">
              <CardTitle className="flex items-center gap-2 text-lg">
                <Info className="h-5 w-5 text-primary" />
                About
              </CardTitle>
            </CardHeader>
            <CardContent>
              <p className="text-sm text-muted-foreground">KaraokeTyper v0.1.0</p>
              <p className="text-sm text-muted-foreground">
                Offline voice dictation for{" "}
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

      {/* Fixed Footer */}
      <div className="flex-none flex items-center justify-end gap-4 px-6 py-4 border-t border-border">
        {saveStatus && (
          <span
            className={cn(
              "text-sm",
              saveStatus.startsWith("Error") ? "text-destructive" : "text-green-500",
            )}
          >
            {saveStatus}
          </span>
        )}
        <Button onClick={handleSave}>Save & Close</Button>
      </div>
    </div>
  );
}

export default SettingsWindow;
