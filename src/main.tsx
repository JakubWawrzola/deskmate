import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import WidgetPanel from "./pages/WidgetPanel";
import "./styles.css";
import { api } from "./api";
import { applyTheme } from "./theme";

// okno "widget" (tauri.conf.json laduje index.html#widget) renderuje panel kafelkow
const isWidget = window.location.hash === "#widget";

// Both windows (main and widget) pick up the chosen theme on start.
api
  .getConfig()
  .then((view) => applyTheme(view.config.theme))
  .catch(() => applyTheme("system"));

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>{isWidget ? <WidgetPanel /> : <App />}</React.StrictMode>,
);
