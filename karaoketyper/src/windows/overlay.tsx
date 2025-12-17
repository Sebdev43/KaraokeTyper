import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import RecordingOverlay from "../components/RecordingOverlay";
import "@/styles/globals.css";

createRoot(document.getElementById("overlay-root")!).render(
  <StrictMode>
    <RecordingOverlay />
  </StrictMode>,
);
