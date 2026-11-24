import "@fontsource/inter/400.css";
import "@fontsource/inter/500.css";
import "@fontsource/inter/600.css";
import "@fontsource-variable/newsreader";
import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./styles/tokens.css";
import "./styles/app.css";

// The phone's own light or dark setting decides, as the desktop's `data-mode` does.
const dark = window.matchMedia("(prefers-color-scheme: dark)");
const applyMode = () => document.documentElement.setAttribute("data-mode", dark.matches ? "dark" : "light");
applyMode();
dark.addEventListener("change", applyMode);

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
