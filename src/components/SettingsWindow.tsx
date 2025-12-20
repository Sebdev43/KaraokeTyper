import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { confirm, message } from "@tauri-apps/plugin-dialog";
import { platform } from "@tauri-apps/plugin-os";
import { relaunch } from "@tauri-apps/plugin-process";
import {
  Brain,
  Cpu,
  Download,
  Gpu,
  Info,
  Keyboard,
  Mic,
  Minus,
  Monitor,
  RefreshCw,
  Settings,
  Square,
  X,
  Zap,
} from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
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
import { cn } from "@/lib/utils";

interface AudioDevice {
  name: string;
  raw_name: string;
  index: number;
}

interface AppConfig {
  shortcut: string;
  audio_device_index: number;
  audio_device_name: string | null;
  model: string;
  key_delay_ms: number;
  language: string;
  stt_provider: string | null;
  model_path: string | null;
}

interface ModelInfo {
  name: string;
  display_name: string;
  size_mb: number;
  url: string;
  installed: boolean;
  description: string;
}

interface DownloadProgress {
  model: string;
  downloaded_mb: number;
  total_mb: number;
  percent: number;
  status: string;
}

interface GpuInfo {
  vendor: string;
  name: string;
  vram_mb: number | null;
  driver_version: string | null;
}

interface GpuDetectionResult {
  success: boolean;
  gpus: GpuInfo[];
  recommended_backend: string;
  install_commands: string[];
  has_nvidia: boolean;
  has_amd: boolean;
  has_apple: boolean;
  has_intel: boolean;
}

// Normalize ONNX Runtime provider names to short names
function normalizeBackend(backend: string): "cuda" | "rocm" | "coreml" | "cpu" {
  const b = backend.toLowerCase();
  if (b.includes("cuda")) return "cuda";
  if (b.includes("migraphx") || b.includes("rocm")) return "rocm";
  if (b.includes("coreml")) return "coreml";
  return "cpu";
}

// Get display name for backend
function getBackendDisplayName(backend: string): string {
  const normalized = normalizeBackend(backend);
  switch (normalized) {
    case "cuda":
      return "NVIDIA CUDA";
    case "rocm":
      return "AMD ROCm";
    case "coreml":
      return "Apple CoreML";
    default:
      return "CPU (Universal)";
  }
}

// Get short name for button labels
function getBackendShortName(backend: string): string {
  const normalized = normalizeBackend(backend);
  switch (normalized) {
    case "cuda":
      return "NVIDIA";
    case "rocm":
      return "AMD";
    case "coreml":
      return "Apple";
    default:
      return "CPU";
  }
}

function SettingsWindow() {
  const [audioDevices, setAudioDevices] = useState<AudioDevice[]>([]);
  const [selectedDevice, setSelectedDevice] = useState<number>(0);
  const [shortcut, setShortcut] = useState("Ctrl+Space");
  const [model, setModel] = useState("sherpa-onnx-whisper-tiny");
  const [language, setLanguage] = useState("fr");
  const [isCapturing, setIsCapturing] = useState(false);
  const [saveStatus, setSaveStatus] = useState<string>("");
  const [isRefreshing, setIsRefreshing] = useState(false);
  const [permissionsInstalled, setPermissionsInstalled] = useState<boolean>(false);
  const [uinputAvailable, setUinputAvailable] = useState<boolean>(false);
  const [pythonAvailable, setPythonAvailable] = useState<boolean>(false);
  const [isInstallingSystem, setIsInstallingSystem] = useState(false);
  const [wireplumberInstalled, setWireplumberInstalled] = useState<boolean>(false);
  const [isInstallingWireplumber, setIsInstallingWireplumber] = useState(false);
  const [gnomeExtInstalled, setGnomeExtInstalled] = useState<boolean>(false);
  const [gnomeExtEnabled, setGnomeExtEnabled] = useState<boolean>(false);
  const [isGnomeWayland, setIsGnomeWayland] = useState<boolean>(false);
  const [isInstallingGnomeExt, setIsInstallingGnomeExt] = useState(false);
  const [installedModels, setInstalledModels] = useState<ModelInfo[]>([]);
  const [downloadingModel, setDownloadingModel] = useState<string | null>(null);
  const [downloadProgress, setDownloadProgress] = useState<number>(0);
  const [currentPlatform, setCurrentPlatform] = useState<string>("linux");
  const inputRef = useRef<HTMLInputElement>(null);

  // GPU & STT Backend state
  const [gpuInfo, setGpuInfo] = useState<GpuDetectionResult | null>(null);
  const [isDetectingGpu, setIsDetectingGpu] = useState(false);
  const [sherpaInstalled, setSherpaInstalled] = useState(false);
  const [isInstallingStt, setIsInstallingStt] = useState(false);
  const [sttInstallProgress, setSttInstallProgress] = useState<string>("");

  // Detect platform on mount
  useEffect(() => {
    try {
      setCurrentPlatform(platform());
    } catch {
      setCurrentPlatform("linux");
    }
  }, []);

  const loadAudioDevices = useCallback(async () => {
    try {
      const devices = await invoke<AudioDevice[]>("get_audio_devices");
      setAudioDevices(devices);
    } catch (error) {
      console.error("Failed to load audio devices:", error);
    }
  }, []);

  const loadSettings = useCallback(async () => {
    try {
      const config = await invoke<AppConfig>("load_settings");
      setShortcut(config.shortcut);
      setSelectedDevice(config.audio_device_index);
      setModel(config.model);
      setLanguage(config.language);
    } catch (error) {
      console.error("Failed to load settings:", error);
    }
  }, []);

  const checkPermissions = useCallback(async () => {
    try {
      const installed = await invoke<boolean>("check_evdev_permissions");
      setPermissionsInstalled(installed);
    } catch (error) {
      console.error("Failed to check permissions:", error);
    }
  }, []);

  const checkUinput = useCallback(async () => {
    try {
      const available = await invoke<boolean>("check_uinput_available");
      setUinputAvailable(available);
    } catch (error) {
      console.error("Failed to check uinput:", error);
    }
  }, []);

  const checkPython = useCallback(async () => {
    try {
      const available = await invoke<boolean>("check_python_available");
      setPythonAvailable(available);
    } catch (error) {
      console.error("Failed to check python:", error);
    }
  }, []);

  const loadInstalledModels = useCallback(async () => {
    try {
      const models = await invoke<ModelInfo[]>("get_installed_models");
      setInstalledModels(models);
    } catch (error) {
      console.error("Failed to load installed models:", error);
    }
  }, []);

  const checkWireplumber = useCallback(async () => {
    try {
      const result = await invoke<{ installed: boolean }>("check_wireplumber_latency_fix");
      setWireplumberInstalled(result.installed);
    } catch (error) {
      console.error("Failed to check wireplumber:", error);
    }
  }, []);

  const checkGnomeExtension = useCallback(async () => {
    try {
      const result = await invoke<{
        installed: boolean;
        enabled: boolean;
        is_gnome: boolean;
        is_wayland: boolean;
      }>("check_focused_window_extension");
      setGnomeExtInstalled(result.installed);
      setGnomeExtEnabled(result.enabled);
      setIsGnomeWayland(result.is_gnome && result.is_wayland);
    } catch (error) {
      console.error("Failed to check GNOME extension:", error);
    }
  }, []);

  const detectGpu = useCallback(async () => {
    setIsDetectingGpu(true);
    try {
      const result = await invoke<GpuDetectionResult>("detect_gpu");
      setGpuInfo(result);
    } catch (error) {
      console.error("Failed to detect GPU:", error);
      setGpuInfo(null);
    } finally {
      setIsDetectingGpu(false);
    }
  }, []);

  const checkSherpaOnnx = useCallback(async () => {
    try {
      const installed = await invoke<boolean>("check_sherpa_onnx_installed");
      setSherpaInstalled(installed);
    } catch (error) {
      console.error("Failed to check sherpa-onnx:", error);
    }
  }, []);

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
      await invoke("download_model", { modelName });
      await loadInstalledModels();
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
    void checkPython();
    void loadInstalledModels();
    void checkWireplumber();
    void checkGnomeExtension();
    void detectGpu();
    void checkSherpaOnnx();
  }, [
    loadAudioDevices,
    loadSettings,
    checkPermissions,
    checkUinput,
    checkPython,
    loadInstalledModels,
    checkWireplumber,
    checkGnomeExtension,
    detectGpu,
    checkSherpaOnnx,
  ]);

  useEffect(() => {
    const unlistenProgress = listen<DownloadProgress>("model-download-progress", (event) => {
      setDownloadProgress(event.payload.percent);
    });

    const unlistenComplete = listen("model-download-complete", () => {
      // Refresh models list when download completes
      void loadInstalledModels();
    });

    const unlistenSttProgress = listen<{ status: string; message: string }>(
      "stt-install-progress",
      (event) => {
        setSttInstallProgress(event.payload.message);
      },
    );

    return () => {
      void unlistenProgress.then((fn) => fn());
      void unlistenComplete.then((fn) => fn());
      void unlistenSttProgress.then((fn) => fn());
    };
  }, [loadInstalledModels]);

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
        stt_provider: null, // Auto-detect
        model_path: modelPath,
      };

      await invoke("save_settings", { newConfig: config });

      setSaveStatus("Settings saved!");
      setTimeout(() => setSaveStatus(""), 2000);

      await getCurrentWindow().hide();
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
      const result = await invoke<string>("install_system_dependencies");

      if (result.includes("restart")) {
        await relaunch();
      } else {
        await message(result, { title: "Succès", kind: "info" });
        await checkPermissions();
        await checkUinput();
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

  const handleInstallSttBackend = async () => {
    if (!pythonAvailable) {
      await message(
        "Python 3.8+ n'est pas installé.\n\nInstallez Python d'abord:\n• Linux: sudo apt install python3 python3-pip python3-venv\n• macOS: brew install python3\n• Windows: Télécharger depuis python.org",
        { title: "Python requis", kind: "warning" },
      );
      return;
    }

    const backend = gpuInfo?.recommended_backend || "CPUExecutionProvider";
    const backendName = getBackendDisplayName(backend);

    const confirmed = await confirm(
      `Installer sherpa-onnx avec ${backendName} ?\n\nCela va installer:\n• sherpa-onnx (reconnaissance vocale)\n• ONNX Runtime optimisé pour ${backendName}\n\nL'installation peut prendre plusieurs minutes.`,
      { title: "Installation STT", kind: "info" },
    );

    if (!confirmed) return;

    setIsInstallingStt(true);
    setSttInstallProgress("Préparation de l'installation...");

    try {
      const result = await invoke<string>("install_stt_dependencies", {
        backend,
      });
      await message(result, { title: "Succès", kind: "info" });
      await checkSherpaOnnx();
    } catch (error) {
      await message(`Échec de l'installation : ${error}`, {
        title: "Erreur",
        kind: "error",
      });
    } finally {
      setIsInstallingStt(false);
      setSttInstallProgress("");
    }
  };

  const handleInstallWireplumber = async () => {
    const confirmed = await confirm(
      "Installer le fix de latence audio WirePlumber ?\n\nCela va :\n• Désactiver la suspension des nœuds audio\n• Éliminer le délai de ~2s au début des enregistrements\n\nNote: Nécessite un redémarrage de WirePlumber.",
      { title: "Fix Latence Audio", kind: "info" },
    );

    if (!confirmed) return;

    setIsInstallingWireplumber(true);

    try {
      const result = await invoke<string>("install_wireplumber_latency_fix");
      await message(result, { title: "Succès", kind: "info" });
      await checkWireplumber();
    } catch (error) {
      await message(`Échec de l'installation : ${error}`, {
        title: "Erreur",
        kind: "error",
      });
    } finally {
      setIsInstallingWireplumber(false);
    }
  };

  const handleRemoveWireplumber = async () => {
    const confirmed = await confirm("Supprimer le fix de latence audio WirePlumber ?", {
      title: "Supprimer Configuration",
      kind: "warning",
    });

    if (!confirmed) return;

    setIsInstallingWireplumber(true);

    try {
      const result = await invoke<string>("remove_wireplumber_latency_fix");
      await message(result, { title: "Succès", kind: "info" });
      await checkWireplumber();
    } catch (error) {
      await message(`Échec de la suppression : ${error}`, {
        title: "Erreur",
        kind: "error",
      });
    } finally {
      setIsInstallingWireplumber(false);
    }
  };

  const handleInstallGnomeExtension = async () => {
    const confirmed = await confirm(
      "Installer l'extension GNOME pour la détection de terminal ?\n\nCela permet de coller automatiquement avec Ctrl+Shift+V dans les terminaux.\n\nNote: Nécessite de se déconnecter/reconnecter pour activer.",
      { title: "Extension Terminal", kind: "info" },
    );

    if (!confirmed) return;

    setIsInstallingGnomeExt(true);

    try {
      const result = await invoke<string>("install_focused_window_extension");
      await message(result, { title: "Succès", kind: "info" });
      await checkGnomeExtension();
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
      const result = await invoke<string>("remove_focused_window_extension");
      await message(result, { title: "Succès", kind: "info" });
      await checkGnomeExtension();
    } catch (error) {
      await message(`Échec de la suppression : ${error}`, {
        title: "Erreur",
        kind: "error",
      });
    } finally {
      setIsInstallingGnomeExt(false);
    }
  };

  const currentWindow = getCurrentWindow();

  const handleMinimize = async (e: React.MouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    try {
      await currentWindow.hide();
    } catch (err) {
      console.error("[TITLEBAR] Minimize error:", err);
    }
  };

  const handleMaximize = async (e: React.MouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    try {
      const isFullscreen = await currentWindow.isFullscreen();
      if (isFullscreen) {
        await currentWindow.setFullscreen(false);
      } else {
        await currentWindow.setFullscreen(true);
      }
    } catch (err) {
      console.error("[TITLEBAR] Fullscreen error:", err);
    }
  };

  const handleClose = async (e: React.MouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    try {
      await currentWindow.hide();
    } catch (err) {
      console.error("[TITLEBAR] Close error:", err);
    }
  };

  const systemDepsInstalled = permissionsInstalled && uinputAvailable;

  return (
    <div className="flex flex-col h-screen overflow-hidden bg-background">
      {/* Fixed Titlebar */}
      <div
        className="shrink-0 flex items-center justify-between h-10 px-3 bg-card border-b border-border select-none"
        data-tauri-drag-region
      >
        <div className="flex items-center gap-2" data-tauri-drag-region>
          <Settings className="h-4 w-4 text-primary" />
          <span className="text-sm font-medium text-foreground">Settings</span>
        </div>
        <div className="flex gap-1" style={{ WebkitAppRegion: "no-drag" } as React.CSSProperties}>
          <Button
            variant="ghost"
            size="icon"
            className="h-7 w-7 hover:bg-muted"
            onClick={handleMinimize}
          >
            <Minus className="h-4 w-4" />
          </Button>
          <Button
            variant="ghost"
            size="icon"
            className="h-7 w-7 hover:bg-muted"
            onClick={handleMaximize}
          >
            <Square className="h-3 w-3" />
          </Button>
          <Button
            variant="ghost"
            size="icon"
            className="h-7 w-7 hover:bg-destructive hover:text-destructive-foreground"
            onClick={handleClose}
          >
            <X className="h-4 w-4" />
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
                  onBlur={handleBlur}
                  readOnly
                  placeholder="Click to set hotkey..."
                  className={cn(
                    "cursor-pointer",
                    isCapturing && "ring-2 ring-primary bg-primary/10",
                  )}
                />
              </div>
            </CardContent>
          </Card>

          {/* GPU & STT Backend */}
          <Card>
            <CardHeader className="pb-3">
              <CardTitle className="flex items-center gap-2 text-lg">
                <Gpu className="h-5 w-5 text-primary" />
                GPU & STT Backend
              </CardTitle>
            </CardHeader>
            <CardContent className="space-y-4">
              {/* GPU Detection */}
              <div className="space-y-3">
                <div className="flex items-center justify-between">
                  <Label>Detected Hardware</Label>
                  <Button variant="ghost" size="sm" onClick={detectGpu} disabled={isDetectingGpu}>
                    <RefreshCw className={cn("h-4 w-4", isDetectingGpu && "animate-spin")} />
                  </Button>
                </div>

                {isDetectingGpu ? (
                  <div className="flex items-center gap-2 text-sm text-muted-foreground">
                    <RefreshCw className="h-4 w-4 animate-spin" />
                    Detecting GPU...
                  </div>
                ) : gpuInfo?.gpus && gpuInfo.gpus.length > 0 ? (
                  <div className="space-y-2">
                    {gpuInfo.gpus.map((gpu) => (
                      <div
                        key={`${gpu.vendor}-${gpu.name}`}
                        className="flex items-center gap-3 p-3 rounded-lg border border-border bg-muted/30"
                      >
                        {gpu.vendor === "NVIDIA" ? (
                          <Zap className="h-5 w-5 text-green-500" />
                        ) : gpu.vendor === "AMD" ? (
                          <Zap className="h-5 w-5 text-red-500" />
                        ) : gpu.vendor === "Apple" ? (
                          <Zap className="h-5 w-5 text-gray-400" />
                        ) : (
                          <Cpu className="h-5 w-5 text-blue-500" />
                        )}
                        <div className="flex-1">
                          <div className="font-medium">{gpu.name}</div>
                          <div className="text-xs text-muted-foreground">
                            {gpu.vendor}
                            {gpu.vram_mb && ` • ${gpu.vram_mb} MB VRAM`}
                            {gpu.driver_version && ` • Driver: ${gpu.driver_version}`}
                          </div>
                        </div>
                        <Badge
                          variant={
                            gpu.vendor === "NVIDIA"
                              ? "default"
                              : gpu.vendor === "AMD"
                                ? "secondary"
                                : "outline"
                          }
                        >
                          {gpu.vendor === "NVIDIA"
                            ? "CUDA"
                            : gpu.vendor === "AMD"
                              ? "ROCm"
                              : gpu.vendor === "Apple"
                                ? "CoreML"
                                : "CPU"}
                        </Badge>
                      </div>
                    ))}
                  </div>
                ) : (
                  <div className="flex items-center gap-2 p-3 rounded-lg border border-border bg-muted/30">
                    <Cpu className="h-5 w-5 text-blue-500" />
                    <div>
                      <div className="font-medium">CPU Only</div>
                      <div className="text-xs text-muted-foreground">No dedicated GPU detected</div>
                    </div>
                  </div>
                )}
              </div>

              <Separator />

              {/* Recommended Backend */}
              <div className="space-y-3">
                <Label>Recommended Backend</Label>
                <div className="flex items-center gap-2">
                  <Badge variant="outline" className="text-sm">
                    {getBackendDisplayName(gpuInfo?.recommended_backend || "")}
                  </Badge>
                  {gpuInfo?.recommended_backend &&
                    normalizeBackend(gpuInfo.recommended_backend) !== "cpu" && (
                      <span className="text-xs text-green-500">GPU acceleration available</span>
                    )}
                </div>
              </div>

              <Separator />

              {/* STT Installation Status */}
              <div className="space-y-3">
                <Label>Speech Recognition Engine (sherpa-onnx)</Label>
                <div className="flex flex-col gap-2">
                  <div className="flex items-center gap-2">
                    <Badge variant={sherpaInstalled ? "default" : "secondary"}>
                      {sherpaInstalled ? "✓" : "○"}
                    </Badge>
                    <span className="text-sm">
                      {sherpaInstalled ? "sherpa-onnx installed" : "sherpa-onnx not installed"}
                    </span>
                  </div>
                  <div className="flex items-center gap-2">
                    <Badge variant={pythonAvailable ? "default" : "secondary"}>
                      {pythonAvailable ? "✓" : "○"}
                    </Badge>
                    <span className="text-sm">Python 3.8+</span>
                  </div>
                </div>

                {sttInstallProgress && (
                  <p className="text-sm text-muted-foreground">{sttInstallProgress}</p>
                )}

                <Button
                  onClick={handleInstallSttBackend}
                  disabled={isInstallingStt || sherpaInstalled}
                  variant={sherpaInstalled ? "outline" : "default"}
                  size="sm"
                >
                  {isInstallingStt
                    ? "Installing..."
                    : sherpaInstalled
                      ? "Installed ✓"
                      : `Install for ${getBackendShortName(gpuInfo?.recommended_backend || "")}`}
                </Button>
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
                Download speech recognition models (sherpa-onnx)
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
                      {modelInfo.name === "sherpa-onnx-whisper-tiny" && (
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

              {/* WirePlumber Latency Fix (Linux only) */}
              {currentPlatform === "linux" && (
                <div className="space-y-3">
                  <Label>Audio Latency Fix</Label>
                  <p className="text-sm text-muted-foreground">
                    {wireplumberInstalled
                      ? "WirePlumber configured - no audio delay"
                      : "Fix ~2s delay at recording start (WirePlumber)"}
                  </p>

                  <div className="flex items-center gap-2">
                    <Badge variant={wireplumberInstalled ? "default" : "secondary"}>
                      {wireplumberInstalled ? "✓" : "○"}
                    </Badge>
                    <span className="text-sm">
                      {wireplumberInstalled
                        ? "Node suspension disabled"
                        : "Audio nodes suspend when idle"}
                    </span>
                  </div>

                  <div className="flex gap-2">
                    <Button
                      onClick={handleInstallWireplumber}
                      disabled={isInstallingWireplumber || wireplumberInstalled}
                      variant={wireplumberInstalled ? "outline" : "default"}
                      size="sm"
                    >
                      {isInstallingWireplumber
                        ? "Installing..."
                        : wireplumberInstalled
                          ? "Installed ✓"
                          : "Install Fix"}
                    </Button>
                    {wireplumberInstalled && (
                      <Button
                        onClick={handleRemoveWireplumber}
                        disabled={isInstallingWireplumber}
                        variant="destructive"
                        size="sm"
                      >
                        Remove
                      </Button>
                    )}
                  </div>
                </div>
              )}
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
