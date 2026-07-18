import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "@fontsource-variable/hanken-grotesk";

import { App } from "./App";
import { ErrorBoundary } from "./components/ErrorBoundary";
import "./codex-shell.css";

const root = document.getElementById("root");

if (!root) {
  throw new Error("Bridge Codex root element is missing");
}

createRoot(root).render(
  <StrictMode>
    <ErrorBoundary>
      <App />
    </ErrorBoundary>
  </StrictMode>,
);
