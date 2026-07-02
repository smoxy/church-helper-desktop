import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { initThemeFromCache } from "./lib/theme";
import "./index.css";

initThemeFromCache();

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>
);
