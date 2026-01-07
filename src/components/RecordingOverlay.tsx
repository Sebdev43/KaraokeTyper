import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { Zap } from "lucide-react";
import { useCallback, useEffect, useRef } from "react";
import { commands } from "@/bindings";
import { getAudioCapture, transcribeAudio } from "@/lib/audioCapture";
import { type AppStatus, useAppStore } from "@/stores";

interface StatusEvent {
  status: AppStatus;
  message: string;
}

// Helper to log to Rust console
const logToRust = (msg: string) => {
  console.log(msg);
  commands.logFromFrontend(msg).catch(() => {});
};

// Guard to prevent duplicate listener setup
let listenersInitialized = false;

function RecordingOverlay() {
  const { status, setStatus } = useAppStore();
  const isRecordingRef = useRef<boolean>(false);
  const audioCapture = useRef(getAudioCapture());

  // Log on mount (only once)
  useEffect(() => {
    logToRust("[Overlay] Component mounted");
  }, []);

  // Handle start recording via Web Audio API
  const handleStartRecording = useCallback(async () => {
    logToRust("[Overlay] Starting Web Audio recording...");
    setStatus("recording");
    isRecordingRef.current = true;

    try {
      await audioCapture.current.startRecording();
      logToRust("[Overlay] Web Audio recording started successfully");
    } catch (error) {
      logToRust(`[Overlay] Failed to start Web Audio recording: ${error}`);
      setStatus("idle");
      isRecordingRef.current = false;
    }
  }, [setStatus]);

  // Handle stop recording and transcription
  const handleStopRecording = useCallback(async () => {
    logToRust("[Overlay] Stopping Web Audio recording...");
    setStatus("processing");
    isRecordingRef.current = false;

    try {
      // Stop recording and get audio data
      const audioData = audioCapture.current.stopRecording();

      if (audioData.length === 0) {
        logToRust("[Overlay] No audio captured");
        setStatus("idle");
        return;
      }

      logToRust(`[Overlay] Got ${audioData.length} samples, sending to backend...`);

      // Send to backend for transcription
      const transcription = await transcribeAudio(audioData);
      logToRust(`[Overlay] Transcription: "${transcription}"`);

      if (transcription?.trim()) {
        // Type the transcription
        const typeResult = await commands.typeText(transcription.trim());
        if (typeResult.status === "ok") {
          logToRust("[Overlay] Text typed successfully");
        } else {
          logToRust(`[Overlay] Failed to type text: ${typeResult.error}`);
        }
      } else {
        logToRust("[Overlay] No speech detected");
      }
    } catch (error) {
      logToRust(`[Overlay] Transcription failed: ${error}`);
    } finally {
      setStatus("idle");
    }
  }, [setStatus]);

  useEffect(() => {
    // Guard against duplicate listener setup
    if (listenersInitialized) {
      logToRust("[Overlay] Listeners already initialized, skipping");
      return;
    }
    listenersInitialized = true;

    const unlisteners: UnlistenFn[] = [];

    const setupListeners = async () => {
      // Listen for status changes from backend
      const unlistenStatus = await listen<StatusEvent>("status-changed", (event) => {
        // Only update status if not controlled by Web Audio
        if (!isRecordingRef.current || event.payload.status !== "recording") {
          setStatus(event.payload.status);
        }
      });
      unlisteners.push(unlistenStatus);

      // Listen for recording started event - trigger Web Audio capture
      const unlistenStarted = await listen("recording-started", () => {
        logToRust("[Overlay] Received recording-started event");
        handleStartRecording();
      });
      unlisteners.push(unlistenStarted);

      // Listen for recording stopped event - stop capture and transcribe
      const unlistenStopped = await listen("recording-stopped", () => {
        logToRust("[Overlay] Received recording-stopped event");
        handleStopRecording();
      });
      unlisteners.push(unlistenStopped);

      logToRust("[Overlay] Event listeners set up");
    };

    setupListeners();

    return () => {
      for (const unlisten of unlisteners) {
        unlisten();
      }
      listenersInitialized = false;
    };
  }, [handleStartRecording, handleStopRecording, setStatus]);

  return (
    <div className="inline-flex items-center gap-2 px-3 py-2 bg-zinc-900/80 backdrop-blur-md rounded-lg border border-zinc-700/40 shadow-lg">
      {/* Idle State - Minimal green dot */}
      {status === "idle" && (
        <>
          <div className="h-2 w-2 rounded-full bg-emerald-500 shadow-sm shadow-emerald-500/40" />
          <span className="text-xs text-zinc-300 font-medium">Prêt</span>
        </>
      )}

      {/* Recording State - Pulsing red dot with waves */}
      {status === "recording" && (
        <>
          <div className="relative flex items-center">
            {/* Pulsing dot */}
            <div className="h-2 w-2 rounded-full bg-red-500 animate-pulse shadow-sm shadow-red-500/60" />
          </div>

          {/* Mini sound wave bars */}
          <div className="flex items-center gap-[2px]">
            <div
              className="w-[2px] bg-red-500 rounded-full overlay-soundwave h-2"
              style={{ animationDelay: "0s" }}
            />
            <div
              className="w-[2px] bg-red-500 rounded-full overlay-soundwave h-2"
              style={{ animationDelay: "0.12s" }}
            />
            <div
              className="w-[2px] bg-red-500 rounded-full overlay-soundwave h-2"
              style={{ animationDelay: "0.24s" }}
            />
            <div
              className="w-[2px] bg-red-500 rounded-full overlay-soundwave h-2"
              style={{ animationDelay: "0.36s" }}
            />
          </div>

          <span className="text-xs text-red-400 font-medium">Écoute...</span>
        </>
      )}

      {/* Processing State - Spinning amber dot */}
      {status === "processing" && (
        <>
          <div className="relative flex items-center">
            <Zap className="h-3 w-3 text-amber-500 animate-pulse" />
          </div>

          <div className="flex items-center gap-[2px]">
            <div
              className="w-[2px] bg-amber-500 rounded-full overlay-shimmer h-2.5"
              style={{ animationDelay: "0s" }}
            />
            <div
              className="w-[2px] bg-amber-500 rounded-full overlay-shimmer h-2.5"
              style={{ animationDelay: "0.15s" }}
            />
            <div
              className="w-[2px] bg-amber-500 rounded-full overlay-shimmer h-2.5"
              style={{ animationDelay: "0.3s" }}
            />
            <div
              className="w-[2px] bg-amber-500 rounded-full overlay-shimmer h-2.5"
              style={{ animationDelay: "0.45s" }}
            />
            <div
              className="w-[2px] bg-amber-500 rounded-full overlay-shimmer h-2.5"
              style={{ animationDelay: "0.6s" }}
            />
          </div>

          <span className="text-xs text-amber-400 font-medium">IA...</span>
        </>
      )}
    </div>
  );
}

export default RecordingOverlay;
