import { listen } from "@tauri-apps/api/event";
import { Loader2, Mic, Square } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { commands } from "@/bindings";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { initUiLanguage, translations, useT } from "@/i18n";
import { cn } from "@/lib/utils";
import "@/styles/globals.css";

interface StatusEvent {
  status: string;
  message: string;
}

function App() {
  const t = useT();
  const [status, setStatus] = useState<string>("idle");
  // Null until something has been said: rendering falls back to the catalogue,
  // so the greeting follows a language change instead of freezing the words
  // picked on mount.
  const [message, setMessage] = useState<string | null>(null);
  const [transcription, setTranscription] = useState<string>("");
  const [isInitialized, setIsInitialized] = useState<boolean>(false);
  const [isRecording, setIsRecording] = useState<boolean>(false);
  const isRecordingRef = useRef<boolean>(false);

  // This window is its own webview, so it reads and follows the interface
  // language itself.
  useEffect(() => {
    let stop: (() => void) | undefined;
    let cancelled = false;
    void initUiLanguage().then((unlisten) => {
      if (cancelled) unlisten();
      else stop = unlisten;
    });
    return () => {
      cancelled = true;
      stop?.();
    };
  }, []);

  useEffect(() => {
    const initApp = async () => {
      try {
        const result = await commands.initStt();
        if (result.status === "ok") {
          setIsInitialized(true);
          setMessage(translations().main.ready);
        } else {
          setMessage(`Error: ${result.error}`);
          console.error("Failed to initialize STT:", result.error);
        }
      } catch (error) {
        setMessage(`Error: ${error}`);
        console.error("Failed to initialize STT:", error);
      }
    };

    initApp();

    let unlisten: (() => void) | undefined;
    const setupListeners = async () => {
      unlisten = await listen<StatusEvent>("status-changed", (event) => {
        setStatus(event.payload.status);
        setMessage(event.payload.message);
      });
    };

    setupListeners();

    // Clean up the Tauri listener on unmount / StrictMode re-run to avoid
    // duplicate "status-changed" handlers stacking up.
    return () => {
      unlisten?.();
    };
  }, []);

  const startRecording = async () => {
    try {
      isRecordingRef.current = true;
      setIsRecording(true);
      const result = await commands.startRecording();
      if (result.status === "error") {
        setMessage(`Error starting recording: ${result.error}`);
        console.error(result.error);
        isRecordingRef.current = false;
        setIsRecording(false);
      }
    } catch (error) {
      setMessage(`Error starting recording: ${error}`);
      console.error(error);
      isRecordingRef.current = false;
      setIsRecording(false);
    }
  };

  const stopRecording = async () => {
    try {
      const result = await commands.stopRecording();
      if (result.status === "ok") {
        const text = result.data;
        setTranscription(text);
        isRecordingRef.current = false;
        setIsRecording(false);

        if (text) {
          await commands.typeText(text);
        }
      } else {
        setMessage(`Error: ${result.error}`);
        console.error(result.error);
        isRecordingRef.current = false;
        setIsRecording(false);
      }
    } catch (error) {
      setMessage(`Error: ${error}`);
      console.error(error);
      isRecordingRef.current = false;
      setIsRecording(false);
    }
  };

  const handleManualRecord = async () => {
    if (isRecording) {
      await stopRecording();
    } else {
      await startRecording();
    }
  };

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
        return t.main.statusRecording;
      case "processing":
        return t.main.statusProcessing;
      case "idle":
        return t.main.statusIdle;
      default:
        return t.main.statusUnknown;
    }
  };

  return (
    <div className="min-h-screen bg-background p-4">
      <Card className="max-w-md mx-auto">
        <CardContent className="p-4 space-y-4">
          {/* Header */}
          <div className="flex items-center justify-between">
            <div className="flex items-center gap-2">
              <Mic className="h-5 w-5 text-primary" />
              <span className="font-semibold text-foreground">KaraokeTyper</span>
            </div>
            <Badge variant={getStatusVariant()} className={cn(isRecording && "animate-pulse")}>
              {status === "processing" && <Loader2 className="h-3 w-3 mr-1 animate-spin" />}
              {getStatusLabel()}
            </Badge>
          </div>

          {/* Status Message */}
          <p className="text-sm text-muted-foreground text-center">
            {message ?? t.main.pressToStart}
          </p>

          {/* Record Button */}
          <Button
            onClick={handleManualRecord}
            disabled={!isInitialized}
            variant={isRecording ? "destructive" : "default"}
            className={cn("w-full", isRecording && "animate-pulse")}
          >
            {isRecording ? (
              <>
                <Square className="h-4 w-4 mr-2" />
                {t.main.stop}
              </>
            ) : (
              <>
                <Mic className="h-4 w-4 mr-2" />
                {t.main.record}
              </>
            )}
          </Button>

          {/* Transcription */}
          {transcription && (
            <div className="p-3 bg-muted rounded-md">
              <p className="text-xs text-muted-foreground mb-1">{t.main.lastTranscription}</p>
              <p className="text-sm text-foreground">{transcription}</p>
            </div>
          )}
        </CardContent>
      </Card>
    </div>
  );
}

export default App;
