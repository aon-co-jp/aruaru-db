import React from "react";
import ReactDOM from "react-dom/client";
import { App } from "@aruaru/admin-core";
import "@aruaru/admin-core/styles.css";

// Web 版エントリ。接続先は VITE_ARUARU_SERVER (ビルド時) か
// 画面の設定 (localStorage) で解決される。
ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>
);
