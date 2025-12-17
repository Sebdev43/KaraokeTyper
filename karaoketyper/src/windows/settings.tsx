import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import SettingsWindow from "../components/SettingsWindow";
import "@/styles/globals.css";

createRoot(document.getElementById("settings-root")!).render(
  <StrictMode>
    <SettingsWindow />
  </StrictMode>,
);
