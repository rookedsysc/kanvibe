import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "@/desktop/renderer/App";
import "@/styles/globals.css";

const container = document.getElementById("root");

if (!container) {
  throw new Error("Renderer root container not found.");
}

createRoot(container).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
