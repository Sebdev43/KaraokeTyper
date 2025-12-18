import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { Loader2 } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { getAudioCapture, transcribeAudio } from "@/lib/audioCapture";
import { cn } from "@/lib/utils";

// Static equalizer bar heights (looks like frozen audio waveform)
const EQUALIZER_BARS = [0.3, 0.7, 0.5, 0.9, 0.4, 0.8, 0.6, 0.95, 0.5, 0.7, 0.4, 0.85, 0.55];

interface StatusEvent {
  status: string;
  message: string;
}

// Helper to log to Rust console
const logToRust = (msg: string) => {
  console.log(msg);
  invoke("log_from_frontend", { message: msg }).catch(() => {});
};

function RecordingOverlay() {
  const [status, setStatus] = useState<string>("idle");
  const isRecordingRef = useRef<boolean>(false);
  const audioCapture = useRef(getAudioCapture());

  // Log on mount
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
  }, []);

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
        await invoke("type_text", { text: transcription.trim() });
        logToRust("[Overlay] Text typed successfully");
      } else {
        logToRust("[Overlay] No speech detected");
      }
    } catch (error) {
      logToRust(`[Overlay] Transcription failed: ${error}`);
    } finally {
      setStatus("idle");
    }
  }, []);

  useEffect(() => {
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
    };
  }, [handleStartRecording, handleStopRecording]);

  return (
    <div className="flex items-center gap-3 px-4 py-3 bg-zinc-900/95 backdrop-blur-sm rounded-xl border border-zinc-700 shadow-2xl">
      {/* Big Recording Dot */}
      <div
        className={cn(
          "h-5 w-5 rounded-full flex-shrink-0 shadow-lg",
          status === "recording" && "bg-red-500 shadow-red-500/50",
          status === "processing" && "bg-amber-500 shadow-amber-500/50",
          status === "idle" && "bg-emerald-500 shadow-emerald-500/50",
          !["recording", "processing", "idle"].includes(status) && "bg-zinc-500",
        )}
      />

      {/* Static Equalizer Bars */}
      {status === "recording" && (
        <div className="flex items-end gap-[3px] h-6">
          {EQUALIZER_BARS.map((height, i) => (
            <div
              key={`bar-${i}-${height}`}
              className="w-[3px] bg-red-500 rounded-full"
              style={{ height: `${height * 24}px` }}
            />
          ))}
        </div>
      )}

      {/* Processing Spinner */}
      {status === "processing" && (
        <div className="flex items-center gap-2">
          <Loader2 className="h-5 w-5 text-amber-500 animate-spin" />
          <span className="text-sm text-zinc-300 font-medium">Transcription...</span>
        </div>
      )}

      {/* Idle State */}
      {status === "idle" && <span className="text-sm text-zinc-400 font-medium">Prêt</span>}
    </div>
  );
}

export default RecordingOverlay;
