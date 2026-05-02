import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "@/desktop/renderer/App";
import "@/styles/globals.css";

function serializeRendererError(error: unknown): string {
  if (error instanceof Error) {
    return error.stack || `${error.name}: ${error.message}`;
  }

  if (typeof error === "string") {
    return error;
  }

  try {
    return JSON.stringify(error);
  } catch {
    return String(error);
  }
}

function installRendererDiagnostics() {
  window.kanvibeDesktop?.logRendererError?.("renderer:bootstrap", {
    href: window.location.href,
    userAgent: window.navigator.userAgent,
  });

  window.addEventListener("error", (event) => {
    window.kanvibeDesktop?.logRendererError?.("renderer:window-error", {
      message: event.message,
      filename: event.filename,
      lineno: event.lineno,
      colno: event.colno,
      error: serializeRendererError(event.error),
    });
  });

  window.addEventListener("unhandledrejection", (event) => {
    window.kanvibeDesktop?.logRendererError?.("renderer:unhandled-rejection", {
      reason: serializeRendererError(event.reason),
    });
  });
}

installRendererDiagnostics();

const container = document.getElementById("root");

if (!container) {
  throw new Error("Renderer root container not found.");
}

createRoot(container).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
