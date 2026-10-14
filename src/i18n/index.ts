import { listen } from "@tauri-apps/api/event";
import { create } from "zustand";
import { commands } from "@/bindings";
import { en } from "./en";
import { type Dict, fr } from "./fr";

export type { Dict };

/** The two codes the backend's `UiLanguage` serialises to, lowercased. */
export type LanguageCode = "fr" | "en";

const CATALOGUES: Record<LanguageCode, Dict> = { fr, en };

/** `UiLanguage` as the backend spells it ("Fr"/"En") → the code used here. */
export function toLanguageCode(value: string | null | undefined): LanguageCode {
  return value?.toLowerCase() === "fr" ? "fr" : "en";
}

/**
 * First paint happens before the backend has answered, so start from the
 * browser locale — the same guess `UiLanguage::from_environment` makes — rather
 * than from a hard-coded language that would flash the wrong one.
 */
function initialLanguage(): LanguageCode {
  if (typeof navigator === "undefined") return "en";
  return navigator.language?.toLowerCase().startsWith("fr") ? "fr" : "en";
}

interface I18nState {
  language: LanguageCode;
  setLanguage: (language: LanguageCode) => void;
}

export const useI18nStore = create<I18nState>()((set) => ({
  language: initialLanguage(),
  setLanguage: (language) => {
    // Screen readers and the browser's own hyphenation read this attribute, so
    // it has to follow the text and not just the first render.
    if (typeof document !== "undefined") document.documentElement.lang = language;
    set({ language });
  },
}));

/** The catalogue for the current language. Re-renders on a change. */
export function useT(): Dict {
  return CATALOGUES[useI18nStore((state) => state.language)];
}

/** The catalogue outside a component (event handlers, async callbacks). */
export function translations(): Dict {
  return CATALOGUES[useI18nStore.getState().language];
}

/**
 * Adopts the stored interface language and follows it afterwards.
 *
 * Every window calls this: the settings window, the main window and the overlay
 * each run in their own webview with their own store, and `ui-language-changed`
 * is what keeps them in step without any of them polling the config.
 */
export async function initUiLanguage(): Promise<() => void> {
  const { setLanguage } = useI18nStore.getState();
  try {
    const result = await commands.loadSettings();
    if (result.status === "ok") {
      setLanguage(toLanguageCode(result.data.ui_language));
    }
  } catch (error) {
    // A failure here only means the window keeps the locale guess above.
    console.error("Failed to read the interface language:", error);
  }
  return await listen<string>("ui-language-changed", (event) => {
    useI18nStore.getState().setLanguage(toLanguageCode(event.payload));
  });
}
