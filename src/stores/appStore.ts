import { create } from "zustand";

export type AppStatus = "idle" | "recording" | "processing" | "error";

interface AppState {
  // Recording state
  status: AppStatus;
  message: string;
  transcription: string;
  isRecording: boolean;

  // Initialization
  isInitialized: boolean;
  initError: string | null;

  // Actions
  setStatus: (status: AppStatus) => void;
  setMessage: (message: string) => void;
  setTranscription: (text: string) => void;
  setIsRecording: (recording: boolean) => void;
  setIsInitialized: (initialized: boolean) => void;
  setInitError: (error: string | null) => void;
  startRecording: () => void;
  stopRecording: () => void;
  reset: () => void;
}

const initialState = {
  status: "idle" as AppStatus,
  message: "Appuyez sur Ctrl+Espace pour dicter",
  transcription: "",
  isRecording: false,
  isInitialized: false,
  initError: null,
};

export const useAppStore = create<AppState>()((set) => ({
  ...initialState,

  setStatus: (status) => set({ status }),

  setMessage: (message) => set({ message }),

  setTranscription: (text) => set({ transcription: text }),

  setIsRecording: (recording) => set({ isRecording: recording }),

  setIsInitialized: (initialized) => set({ isInitialized: initialized }),

  setInitError: (error) => set({ initError: error }),

  startRecording: () =>
    set({
      status: "recording",
      isRecording: true,
      message: "Écoute en cours...",
    }),

  stopRecording: () =>
    set({
      status: "processing",
      isRecording: false,
      message: "Transcription...",
    }),

  reset: () => set(initialState),
}));
