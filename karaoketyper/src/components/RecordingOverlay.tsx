import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { Loader2, Mic } from "lucide-react";
import { useEffect, useRef, useState, useCallback } from "react";
import { Badge } from "@/components/ui/badge";
import { cn } from "@/lib/utils";
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
  const [isRecording, setIsRecording] = useState<boolean>(false);
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
    setIsRecording(true);
    isRecordingRef.current = true;

    try {
      await audioCapture.current.startRecording();
      logToRust("[Overlay] Web Audio recording started successfully");
    } catch (error) {
      logToRust(`[Overlay] Failed to start Web Audio recording: ${error}`);
      setStatus("idle");
      setIsRecording(false);
      isRecordingRef.current = false;
    }
  }, []);

  // Handle stop recording and transcription
  const handleStopRecording = useCallback(async () => {
    logToRust("[Overlay] Stopping Web Audio recording...");
    setStatus("processing");
    setIsRecording(false);
    isRecordingRef.current = false;

    try {
      // Stop recording and get audio data
      const audioData = audioCapture.current.stopRecording();

      if (audioData.length === 0) {
        logToRust("[Overlay] No audio captured");
        setStatus("idle");
        return;
      }

      logToRust(
        `[Overlay] Got ${audioData.length} samples, sending to backend...`,
      );

      // Send to backend for transcription
      const transcription = await transcribeAudio(audioData);
      logToRust(`[Overlay] Transcription: "${transcription}"`);

      if (transcription && transcription.trim()) {
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
      const unlistenStatus = await listen<StatusEvent>(
        "status-changed",
        (event) => {
          // Only update status if not controlled by Web Audio
          if (!isRecordingRef.current || event.payload.status !== "recording") {
            setStatus(event.payload.status);
          }
        },
      );
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

  const getStatusVariant = (): "default" | "secondary" | "destructive" => {
    switch (status) {
      case "recording":
        return "destructive";
      case "processing":
        return "secondary";
      case "idle":
        return "default";
      default:
        return "secondary";
    }
  };

  const getStatusLabel = () => {
    switch (status) {
      case "recording":
        return "Recording...";
      case "processing":
        return "Processing...";
      case "idle":
        return "Ready";
      default:
        return "Ready";
    }
  };

  return (
    <div className="flex items-center gap-2 p-2 bg-background/80 backdrop-blur-sm rounded-lg border border-border shadow-lg">
      <div
        className={cn(
          "h-3 w-3 rounded-full",
          status === "recording" && "bg-destructive animate-pulse",
          status === "processing" && "bg-yellow-500",
          status === "idle" && "bg-green-500",
          !["recording", "processing", "idle"].includes(status) &&
            "bg-muted-foreground",
        )}
      />
      <Badge
        variant={getStatusVariant()}
        className={cn("text-xs", isRecording && "animate-pulse")}
      >
        {status === "recording" && <Mic className="h-3 w-3 mr-1" />}
        {status === "processing" && (
          <Loader2 className="h-3 w-3 mr-1 animate-spin" />
        )}
        {getStatusLabel()}
      </Badge>
    </div>
  );
}

export default RecordingOverlay;
