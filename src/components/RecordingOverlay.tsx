import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useEffect, useLayoutEffect, useState } from "react";
import { initUiLanguage, useT } from "@/i18n";

type OverlayState = "idle" | "recording" | "processing";

// Length of the fade-out. Must stay in sync with two other places, because all three
// describe the same animation: the `transition: opacity` in src/styles/overlay.css,
// and the delay before `window.hide()` in hide_overlay_animated (src-tauri/src/lib.rs).
const FADE_OUT_MS = 300;

function RecordingOverlay() {
  const t = useT();
  const [isVisible, setIsVisible] = useState(false);
  const [state, setState] = useState<OverlayState>("recording");
  // The overlay lives in a webview that is never destroyed, so anything rendered here
  // keeps running even while the window is hidden — and every state below animates on
  // an `infinite` keyframe. Rendering the content only while it can actually be seen
  // is what stops those animations from burning CPU in the background from boot.
  const [showContent, setShowContent] = useState(false);

  // The overlay is its own webview: it has to read the interface language and
  // follow later changes on its own.
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

  // Mount the content as soon as the overlay starts fading in, and keep it mounted
  // for the whole fade-out — unmounting on the first hidden frame would leave the
  // window blank for 300ms instead of fading it. The brief `isVisible` dip in the
  // WebKitGTK repaint trick above cancels its own pending timer on the next frame.
  // useLayoutEffect on purpose: with a plain effect the browser can paint the frame
  // where the container is already fading in but the content has not mounted yet,
  // which shows as an empty box for one frame at the start of every dictation.
  useLayoutEffect(() => {
    if (isVisible) {
      setShowContent(true);
      return;
    }
    const timer = setTimeout(() => setShowContent(false), FADE_OUT_MS);
    return () => clearTimeout(timer);
  }, [isVisible]);

  return (
    <div className={`overlay-container ${isVisible ? "fade-in" : ""}`}>
      {showContent && (
        <>
          {/* Idle State */}
          {state === "idle" && (
            <>
              <div className="dot dot-green" />
              <span className="overlay-text">{t.overlay.ready}</span>
            </>
          )}

          {/* Recording State */}
          {state === "recording" && (
            <>
              <div className="dot dot-red dot-pulse" />
              <div className="bars-container">
                <div className="bar bar-red overlay-soundwave" style={{ animationDelay: "0s" }} />
                <div
                  className="bar bar-red overlay-soundwave"
                  style={{ animationDelay: "0.12s" }}
                />
                <div
                  className="bar bar-red overlay-soundwave"
                  style={{ animationDelay: "0.15s" }}
                />
                <div
                  className="bar bar-red overlay-soundwave"
                  style={{ animationDelay: "0.24s" }}
                />
                <div
                  className="bar bar-red overlay-soundwave"
                  style={{ animationDelay: "0.36s" }}
                />
              </div>
              <span className="overlay-text text-red">{t.overlay.listening}</span>
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
                aria-label={t.overlay.processing}
              >
                <path d="M13 2L3 14h9l-1 8 10-12h-9l1-8z" />
              </svg>
              <div className="bars-container">
                <div className="bar bar-amber overlay-shimmer" style={{ animationDelay: "0s" }} />
                <div
                  className="bar bar-amber overlay-shimmer"
                  style={{ animationDelay: "0.15s" }}
                />
                <div className="bar bar-amber overlay-shimmer" style={{ animationDelay: "0.3s" }} />
                <div
                  className="bar bar-amber overlay-shimmer"
                  style={{ animationDelay: "0.45s" }}
                />
                <div className="bar bar-amber overlay-shimmer" style={{ animationDelay: "0.6s" }} />
              </div>
              <span className="overlay-text text-amber">{t.overlay.ai}</span>
            </>
          )}
        </>
      )}
    </div>
  );
}

export default RecordingOverlay;
