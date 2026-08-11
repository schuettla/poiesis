import React from "react";
import ReactDOM from "react-dom/client";

// Self-hosted fonts (offline-capable — no CDN). PRD §5.2 type roles.
import "@fontsource-variable/newsreader";
import "@fontsource/inter/400.css";
import "@fontsource/inter/500.css";
import "@fontsource/inter/600.css";
import "@fontsource/jetbrains-mono/400.css";
import "@fontsource/jetbrains-mono/500.css";

import "./styles/tokens.css";
import "./styles/global.css";
import "./styles/menu.css";

import App from "./App";

// Desktop-app feel: no browser right-click menu anywhere. Text fields still
// get their native context menu (cut/copy/paste), since that's the one place
// a webview's default menu is actually expected behavior.
window.addEventListener("contextmenu", (e) => {
  const target = e.target as HTMLElement | null;
  const editable =
    target?.closest('input, textarea, [contenteditable="true"]') !== null;
  if (!editable) e.preventDefault();
});

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>
);
