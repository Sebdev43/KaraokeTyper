import { create } from "zustand";
import { persist } from "zustand/middleware";
import type { AudioDevice, ModelInfo } from "@/bindings";

interface SettingsState {
  // Audio settings
  audioDevices: AudioDevice[];
  selectedDevice: number;
  selectedDeviceName: string;

  // STT settings
  model: string;
  language: string;

  // Keyboard shortcut
  shortcut: string;

  // Model management
  installedModels: ModelInfo[];
  downloadingModel: string | null;
  downloadProgress: number;

  // App behavior
  autostart: boolean;

  // UI states
  isRefreshing: boolean;
  saveStatus: string;

  // Actions
  setAudioDevices: (devices: AudioDevice[]) => void;
  setSelectedDevice: (index: number, name?: string) => void;
  setModel: (model: string) => void;
  setLanguage: (language: string) => void;
  setShortcut: (shortcut: string) => void;
  setInstalledModels: (models: ModelInfo[]) => void;
  setDownloadingModel: (model: string | null) => void;
  setDownloadProgress: (progress: number) => void;
  setAutostart: (enabled: boolean) => void;
  setIsRefreshing: (refreshing: boolean) => void;
  setSaveStatus: (status: string) => void;
  reset: () => void;
}

const initialState = {
  audioDevices: [],
  selectedDevice: 0,
  selectedDeviceName: "",
  model: "ggml-small",
  language: "fr",
  shortcut: "Ctrl+Space",
  installedModels: [],
  downloadingModel: null,
  downloadProgress: 0,
  autostart: false,
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

      setModel: (model) => set({ model }),

      setLanguage: (language) => set({ language }),

      setShortcut: (shortcut) => set({ shortcut }),

      setInstalledModels: (models) => set({ installedModels: models }),

      setDownloadingModel: (model) => set({ downloadingModel: model }),

      setDownloadProgress: (progress) => set({ downloadProgress: progress }),

      setAutostart: (enabled) => set({ autostart: enabled }),

      setIsRefreshing: (refreshing) => set({ isRefreshing: refreshing }),

      setSaveStatus: (status) => set({ saveStatus: status }),

      reset: () => set(initialState),
    }),
    {
      name: "karaoketyper-settings",
      partialize: (state) => ({
        selectedDevice: state.selectedDevice,
        selectedDeviceName: state.selectedDeviceName,
        model: state.model,
        language: state.language,
        shortcut: state.shortcut,
        autostart: state.autostart,
      }),
    },
  ),
);
