import { create } from "zustand";

type Platform = "linux" | "windows" | "macos" | "unknown";

interface SystemState {
  // Platform info
  currentPlatform: Platform;

  // Linux-specific permissions
  permissionsInstalled: boolean;
  uinputAvailable: boolean;

  // GNOME extension state (Wayland)
  gnomeExtInstalled: boolean;
  gnomeExtEnabled: boolean;
  isGnomeWayland: boolean;

  // Loading states
  isInstallingSystem: boolean;
  isInstallingGnomeExt: boolean;
  isTogglingAutostart: boolean;

  // Actions
  setPlatform: (platform: Platform) => void;
  setPermissionsInstalled: (installed: boolean) => void;
  setUinputAvailable: (available: boolean) => void;
  setGnomeExtInstalled: (installed: boolean) => void;
  setGnomeExtEnabled: (enabled: boolean) => void;
  setIsGnomeWayland: (isGnome: boolean) => void;
  setIsInstallingSystem: (installing: boolean) => void;
  setIsInstallingGnomeExt: (installing: boolean) => void;
  setIsTogglingAutostart: (toggling: boolean) => void;
  reset: () => void;
}

const initialState = {
  currentPlatform: "unknown" as Platform,
  permissionsInstalled: false,
  uinputAvailable: false,
  gnomeExtInstalled: false,
  gnomeExtEnabled: false,
  isGnomeWayland: false,
  isInstallingSystem: false,
  isInstallingGnomeExt: false,
  isTogglingAutostart: false,
};

export const useSystemStore = create<SystemState>()((set) => ({
  ...initialState,

  setPlatform: (platform) => set({ currentPlatform: platform }),

  setPermissionsInstalled: (installed) => set({ permissionsInstalled: installed }),

  setUinputAvailable: (available) => set({ uinputAvailable: available }),

  setGnomeExtInstalled: (installed) => set({ gnomeExtInstalled: installed }),

  setGnomeExtEnabled: (enabled) => set({ gnomeExtEnabled: enabled }),

  setIsGnomeWayland: (isGnome) => set({ isGnomeWayland: isGnome }),

  setIsInstallingSystem: (installing) => set({ isInstallingSystem: installing }),

  setIsInstallingGnomeExt: (installing) => set({ isInstallingGnomeExt: installing }),

  setIsTogglingAutostart: (toggling) => set({ isTogglingAutostart: toggling }),

  reset: () => set(initialState),
}));
