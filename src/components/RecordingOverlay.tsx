import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useEffect, useState } from "react";

type OverlayState = "idle" | "recording" | "processing";

function RecordingOverlay() {
  const [isVisible, setIsVisible] = useState(false);
  const [state, setState] = useState<OverlayState>("recording");

  useEffect(() => {
    const unlisteners: UnlistenFn[] = [];

    const setupListeners = async () => {
      // show-overlay: update state + trigger fade-in
      // Force repaint on state change to clear WebKitGTK transparent buffer
      const unlistenShow = await listen<string>("show-overlay", (event) => {
        const newState = event.payload as OverlayState;
        setState((prev) => {
          if (prev !== newState && prev !== "idle") {
            // Briefly hide to force WebKitGTK to clear the old frame
            setIsVisible(false);
            requestAnimationFrame(() => {
              requestAnimationFrame(() => {
                setIsVisible(true);
              });
            });
          } else {
            setIsVisible(true);
          }
          return newState;
        });
      });
      unlisteners.push(unlistenShow);

      // hide-overlay: trigger fade-out (CSS transition handles the animation)
      const unlistenHide = await listen("hide-overlay", () => {
        setIsVisible(false);
      });
      unlisteners.push(unlistenHide);
    };

    setupListeners();

    return () => {
      for (const unlisten of unlisteners) {
        unlisten();
      }
    };
  }, []);

  return (
    <div className={`overlay-container ${isVisible ? "fade-in" : ""}`}>
      {/* Idle State */}
      {state === "idle" && (
        <>
          <div className="dot dot-green" />
          <span className="overlay-text">Prêt</span>
        </>
      )}

      {/* Recording State */}
      {state === "recording" && (
        <>
          <div className="dot dot-red dot-pulse" />
          <div className="bars-container">
            <div className="bar bar-red overlay-soundwave" style={{ animationDelay: "0s" }} />
            <div className="bar bar-red overlay-soundwave" style={{ animationDelay: "0.12s" }} />
            <div className="bar bar-red overlay-soundwave" style={{ animationDelay: "0.15s" }} />
            <div className="bar bar-red overlay-soundwave" style={{ animationDelay: "0.24s" }} />
            <div className="bar bar-red overlay-soundwave" style={{ animationDelay: "0.36s" }} />
          </div>
          <span className="overlay-text text-red">Écoute...</span>
        </>
      )}

      {/* Processing State */}
      {state === "processing" && (
        <>
          <svg
            className="icon-zap"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="2"
            role="img"
            aria-label="Traitement"
          >
            <path d="M13 2L3 14h9l-1 8 10-12h-9l1-8z" />
          </svg>
          <div className="bars-container">
            <div className="bar bar-amber overlay-shimmer" style={{ animationDelay: "0s" }} />
            <div className="bar bar-amber overlay-shimmer" style={{ animationDelay: "0.15s" }} />
            <div className="bar bar-amber overlay-shimmer" style={{ animationDelay: "0.3s" }} />
            <div className="bar bar-amber overlay-shimmer" style={{ animationDelay: "0.45s" }} />
            <div className="bar bar-amber overlay-shimmer" style={{ animationDelay: "0.6s" }} />
          </div>
          <span className="overlay-text text-amber">IA...</span>
        </>
      )}
    </div>
  );
}

export default RecordingOverlay;
