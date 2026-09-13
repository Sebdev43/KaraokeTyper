import { create } from "zustand";
import { persist } from "zustand/middleware";
import type {
  AudioDevice,
  EngineType,
  GpuInfo,
  GpuStatus,
  ModelInfo,
  ModelUnloadTimeout,
  OllamaStatus,
  PasteMethod,
} from "@/bindings";

interface SettingsState {
  // Audio settings
  audioDevices: AudioDevice[];
  selectedDevice: number;
  selectedDeviceName: string;

  // STT settings
  sttBackend: EngineType;
  model: string;
  language: string;
  hybridEngineEnabled: boolean;
  hybridThresholdSecs: number;

  // Keyboard shortcut
  shortcut: string;

  // Model management
  installedModels: ModelInfo[];
  downloadingModel: string | null;
  downloadProgress: number;

  // App behavior
  autostart: boolean;

  // GPU acceleration
  gpuInfo: GpuInfo | null;
  gpuStatus: GpuStatus | null;
  gpuDeviceIndex: number | null;
  useGpuAcceleration: boolean;

  // Audio feedback
  audioFeedbackEnabled: boolean;
  audioFeedbackVolume: number;

  // Model unload
  modelUnloadTimeout: ModelUnloadTimeout;

  // Paste method
  pasteMethod: PasteMethod;

  // Text processing
  customWords: string[];
  customWordsThreshold: number;
  fillerRemovalEnabled: boolean;
  stutterCollapseEnabled: boolean;

  // Ollama
  ollamaEnabled: boolean;
  ollamaUrl: string;
  ollamaModel: string;
  ollamaStatus: OllamaStatus | null;

  // UI states
  isRefreshing: boolean;
  saveStatus: string;

  // Actions
  setAudioDevices: (devices: AudioDevice[]) => void;
  setSelectedDevice: (index: number, name?: string) => void;
  setSttBackend: (backend: EngineType) => void;
  setModel: (model: string) => void;
  setLanguage: (language: string) => void;
  setHybridEngineEnabled: (enabled: boolean) => void;
  setHybridThresholdSecs: (seconds: number) => void;
  setShortcut: (shortcut: string) => void;
  setInstalledModels: (models: ModelInfo[]) => void;
  setDownloadingModel: (model: string | null) => void;
  setDownloadProgress: (progress: number) => void;
  setAutostart: (enabled: boolean) => void;
  setGpuInfo: (info: GpuInfo | null) => void;
  setGpuStatus: (status: GpuStatus | null) => void;
  setGpuDeviceIndex: (index: number | null) => void;
  setUseGpuAcceleration: (enabled: boolean) => void;
  setAudioFeedbackEnabled: (enabled: boolean) => void;
  setAudioFeedbackVolume: (volume: number) => void;
  setModelUnloadTimeout: (timeout: ModelUnloadTimeout) => void;
  setPasteMethod: (method: PasteMethod) => void;
  setCustomWords: (words: string[]) => void;
  setCustomWordsThreshold: (threshold: number) => void;
  setFillerRemovalEnabled: (enabled: boolean) => void;
  setStutterCollapseEnabled: (enabled: boolean) => void;
  setOllamaEnabled: (enabled: boolean) => void;
  setOllamaUrl: (url: string) => void;
  setOllamaModel: (model: string) => void;
  setOllamaStatus: (status: OllamaStatus | null) => void;
  setIsRefreshing: (refreshing: boolean) => void;
  setSaveStatus: (status: string) => void;
  reset: () => void;
}

const initialState = {
  audioDevices: [],
  selectedDevice: 0,
  selectedDeviceName: "",
  sttBackend: "whisper" as EngineType,
  model: "ggml-small",
  language: "fr",
  hybridEngineEnabled: false,
  hybridThresholdSecs: 20,
  shortcut: "Ctrl+Space",
  installedModels: [],
  downloadingModel: null,
  downloadProgress: 0,
  autostart: false,
  gpuInfo: null,
  gpuStatus: null,
  gpuDeviceIndex: null,
  useGpuAcceleration: false, // CPU by default for safety
  audioFeedbackEnabled: true,
  audioFeedbackVolume: 0.7,
  modelUnloadTimeout: "Never" as ModelUnloadTimeout,
  pasteMethod: "Direct" as PasteMethod,
  customWords: [] as string[],
  customWordsThreshold: 0.3,
  fillerRemovalEnabled: true,
  stutterCollapseEnabled: true,
  ollamaEnabled: false,
  ollamaUrl: "http://localhost:11434",
  ollamaModel: "mistral",
  ollamaStatus: null,
  isRefreshing: false,
  saveStatus: "",
};

export const useSettingsStore = create<SettingsState>()(
  persist(
    (set) => ({
      ...initialState,

      setAudioDevices: (devices) => set({ audioDevices: devices }),

      setSelectedDevice: (index, name) =>
        set({
          selectedDevice: index,
          ...(name !== undefined && { selectedDeviceName: name }),
        }),

      setSttBackend: (backend) => set({ sttBackend: backend }),

      setModel: (model) => set({ model }),

      setLanguage: (language) => set({ language }),

      setHybridEngineEnabled: (enabled) => set({ hybridEngineEnabled: enabled }),

      setHybridThresholdSecs: (seconds) => set({ hybridThresholdSecs: seconds }),

      setShortcut: (shortcut) => set({ shortcut }),

      setInstalledModels: (models) => set({ installedModels: models }),

      setDownloadingModel: (model) => set({ downloadingModel: model }),

      setDownloadProgress: (progress) => set({ downloadProgress: progress }),

      setAutostart: (enabled) => set({ autostart: enabled }),

      setGpuInfo: (info) => set({ gpuInfo: info }),

      setGpuStatus: (status) => set({ gpuStatus: status }),

      setGpuDeviceIndex: (index) => set({ gpuDeviceIndex: index }),

      setUseGpuAcceleration: (enabled) => set({ useGpuAcceleration: enabled }),

      setAudioFeedbackEnabled: (enabled) => set({ audioFeedbackEnabled: enabled }),
      setAudioFeedbackVolume: (volume) => set({ audioFeedbackVolume: volume }),
      setModelUnloadTimeout: (timeout) => set({ modelUnloadTimeout: timeout }),
      setPasteMethod: (method) => set({ pasteMethod: method }),
      setCustomWords: (words) => set({ customWords: words }),
      setCustomWordsThreshold: (threshold) => set({ customWordsThreshold: threshold }),
      setFillerRemovalEnabled: (enabled) => set({ fillerRemovalEnabled: enabled }),
      setStutterCollapseEnabled: (enabled) => set({ stutterCollapseEnabled: enabled }),
      setOllamaEnabled: (enabled) => set({ ollamaEnabled: enabled }),
      setOllamaUrl: (url) => set({ ollamaUrl: url }),
      setOllamaModel: (model) => set({ ollamaModel: model }),
      setOllamaStatus: (status) => set({ ollamaStatus: status }),

      setIsRefreshing: (refreshing) => set({ isRefreshing: refreshing }),

      setSaveStatus: (status) => set({ saveStatus: status }),

      reset: () => set(initialState),
    }),
    {
      name: "karaoketyper-settings",
      partialize: (state) => ({
        selectedDevice: state.selectedDevice,
        selectedDeviceName: state.selectedDeviceName,
        sttBackend: state.sttBackend,
        model: state.model,
        language: state.language,
        hybridEngineEnabled: state.hybridEngineEnabled,
        hybridThresholdSecs: state.hybridThresholdSecs,
        shortcut: state.shortcut,
        autostart: state.autostart,
        useGpuAcceleration: state.useGpuAcceleration,
        gpuDeviceIndex: state.gpuDeviceIndex,
        audioFeedbackEnabled: state.audioFeedbackEnabled,
        audioFeedbackVolume: state.audioFeedbackVolume,
        modelUnloadTimeout: state.modelUnloadTimeout,
        pasteMethod: state.pasteMethod,
        ollamaEnabled: state.ollamaEnabled,
        ollamaUrl: state.ollamaUrl,
        ollamaModel: state.ollamaModel,
      }),
    },
  ),
);
