import React from "react";
import ReactDOM from "react-dom/client";
import { App } from "@aruaru/admin-core";
import "@aruaru/admin-core/styles.css";

// Windows インストール型 PWA エントリ。UI コアは web 版と完全共有。
// データ取得は core の fetch ベース invoke シムが Cosmo Router / aruaru-server を叩く。
// vite-plugin-pwa が Service Worker を自動登録 (registerType: autoUpdate)。
ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>
);
