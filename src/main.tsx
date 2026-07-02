import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { initThemeFromCache } from "./lib/theme";
import { initLanguageFromCache } from "./lib/i18n";
import "./index.css";

initThemeFromCache();
initLanguageFromCache();

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>
);
