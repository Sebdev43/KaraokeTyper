import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { Zap } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { getAudioCapture, transcribeAudio } from "@/lib/audioCapture";

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

            {/* Outer ring */}
            <div className="absolute inset-0 h-2 w-2 rounded-full bg-red-500/30 animate-ping" />
          </div>

          {/* Mini sound wave bars */}
          <div className="flex items-center gap-[2px]">
            {[0, 1, 2, 3].map((i) => (
              <div
                key={`wave-${i}`}
                className="w-[2px] bg-red-500 rounded-full"
                style={{
                  animation: "soundwave 0.6s ease-in-out infinite",
                  animationDelay: `${i * 0.12}s`,
                  height: "8px",
                }}
              />
            ))}
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
            {[0, 1, 2, 3, 4].map((i) => (
              <div
                key={`shimmer-${i}`}
                className="w-[2px] bg-amber-500 rounded-full"
                style={{
                  animation: "shimmer 1s ease-in-out infinite",
                  animationDelay: `${i * 0.15}s`,
                  height: "10px",
                }}
              />
            ))}
          </div>

          <span className="text-xs text-amber-400 font-medium">IA...</span>
        </>
      )}

      {/* Minimal CSS animations */}
      <style>{`
        @keyframes soundwave {
          0%, 100% {
            height: 6px;
            opacity: 0.5;
          }
          50% {
            height: 14px;
            opacity: 1;
          }
        }

        @keyframes shimmer {
          0%, 100% {
            height: 8px;
            opacity: 0.4;
          }
          50% {
            height: 14px;
            opacity: 1;
          }
        }
      `}</style>
    </div>
  );
}

export default RecordingOverlay;
