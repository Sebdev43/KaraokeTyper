import { create } from "zustand";
import { translations } from "@/i18n";

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

// A function, not a constant: the message depends on the interface language,
// which is not known yet when this module is first evaluated.
const initialState = () => ({
  status: "idle" as AppStatus,
  message: translations().main.pressToDictate,
  transcription: "",
  isRecording: false,
  isInitialized: false,
  initError: null as string | null,
});

export const useAppStore = create<AppState>()((set) => ({
  ...initialState(),

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
      message: translations().main.listening,
    }),

  stopRecording: () =>
    set({
      status: "processing",
      isRecording: false,
      message: translations().main.transcribing,
    }),

  reset: () => set(initialState()),
}));
