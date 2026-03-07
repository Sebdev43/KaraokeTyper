import { listen } from "@tauri-apps/api/event";
import { Loader2, Mic, Square } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { commands } from "@/bindings";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { cn } from "@/lib/utils";
import "@/styles/globals.css";

interface StatusEvent {
  status: string;
  message: string;
}

function App() {
  const [status, setStatus] = useState<string>("idle");
  const [message, setMessage] = useState<string>(
    "Appuyez sur Ctrl+Espace pour commencer",
  );
  const [transcription, setTranscription] = useState<string>("");
  const [isInitialized, setIsInitialized] = useState<boolean>(false);
  const [isRecording, setIsRecording] = useState<boolean>(false);
  const isRecordingRef = useRef<boolean>(false);

  useEffect(() => {
    const initApp = async () => {
      try {
        const result = await commands.initStt();
        if (result.status === "ok") {
          setIsInitialized(true);
          setMessage("Prêt ! Appuyez sur Ctrl+Espace pour dicter");
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

    const setupListeners = async () => {
      await listen<StatusEvent>("status-changed", (event) => {
        setStatus(event.payload.status);
        setMessage(event.payload.message);
      });
    };

    setupListeners();
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
        return "Enregistrement";
      case "processing":
        return "Traitement";
      case "idle":
        return "Prêt";
      default:
        return "Inconnu";
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
              <span className="font-semibold text-foreground">
                KaraokeTyper
              </span>
            </div>
            <Badge
              variant={getStatusVariant()}
              className={cn(isRecording && "animate-pulse")}
            >
              {status === "processing" && (
                <Loader2 className="h-3 w-3 mr-1 animate-spin" />
              )}
              {getStatusLabel()}
            </Badge>
          </div>

          {/* Status Message */}
          <p className="text-sm text-muted-foreground text-center">{message}</p>

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
                Arrêter
              </>
            ) : (
              <>
                <Mic className="h-4 w-4 mr-2" />
                Enregistrer
              </>
            )}
          </Button>

          {/* Transcription */}
          {transcription && (
            <div className="p-3 bg-muted rounded-md">
              <p className="text-xs text-muted-foreground mb-1">
                Dernière transcription :
              </p>
              <p className="text-sm text-foreground">{transcription}</p>
            </div>
          )}
        </CardContent>
      </Card>
    </div>
  );
}

export default App;
