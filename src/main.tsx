import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import WidgetPanel from "./pages/WidgetPanel";
import "./styles.css";

// okno "widget" (tauri.conf.json laduje index.html#widget) renderuje panel kafelkow
const isWidget = window.location.hash === "#widget";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>{isWidget ? <WidgetPanel /> : <App />}</React.StrictMode>,
);
