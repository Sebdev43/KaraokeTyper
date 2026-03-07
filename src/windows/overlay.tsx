import { createRoot } from "react-dom/client";
import RecordingOverlay from "../components/RecordingOverlay";
import "@/styles/overlay.css";

createRoot(document.getElementById("overlay-root")!).render(
  <RecordingOverlay />,
);
