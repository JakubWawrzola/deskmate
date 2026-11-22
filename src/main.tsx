import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import WidgetPanel from "./pages/WidgetPanel";
import "./styles.css";
import { api } from "./api";
import { setLanguage } from "./i18n";
import { PlatformProvider } from "./platform";
import { applyTheme } from "./theme";

// okno "widget" (tauri.conf.json laduje index.html#widget) renderuje panel kafelkow
const isWidget = window.location.hash === "#widget";

function render() {
  ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
    <React.StrictMode>
      <PlatformProvider>{isWidget ? <WidgetPanel /> : <App />}</PlatformProvider>
    </React.StrictMode>,
  );
}

// Both windows (main and widget) pick up the chosen theme and language on
// start. The first paint waits for the language, so no English flashes by.
api
  .getConfig()
  .then((view) => {
    applyTheme(view.config.theme);
    setLanguage(view.config.language);
  })
  .catch(() => {
    applyTheme("system");
    setLanguage("system");
  })
  .finally(render);
