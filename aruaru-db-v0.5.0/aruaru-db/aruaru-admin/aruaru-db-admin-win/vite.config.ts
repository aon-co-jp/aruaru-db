import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { VitePWA } from "vite-plugin-pwa";

// Tauri を廃止し、インストール型 PWA として配布する。
// - Service Worker でオフライン起動 (アプリシェルをキャッシュ)
// - manifest により Windows/Chrome/Edge から「インストール」可能
// - Web コア (@aruaru/admin-core) は web 版と完全共有
export default defineConfig({
  plugins: [
    react(),
    VitePWA({
      registerType: "autoUpdate",
      includeAssets: ["favicon.svg"],
      manifest: {
        name: "aruaru-DB Admin",
        short_name: "aruaru-admin",
        description: "aruaru-DB 監視・管理アプリ (PostgreSQL + aruaru-db Dual)",
        lang: "ja",
        theme_color: "#0b0f14",
        background_color: "#0b0f14",
        display: "standalone",
        start_url: "/",
        icons: [
          { src: "icons/icon-192.png", sizes: "192x192", type: "image/png" },
          { src: "icons/icon-512.png", sizes: "512x512", type: "image/png" },
          { src: "icons/icon-512-maskable.png", sizes: "512x512", type: "image/png", purpose: "maskable" }
        ]
      },
      workbox: {
        globPatterns: ["**/*.{js,css,html,svg,png,woff2}"],
        // 管理 API/GraphQL はネットワーク優先 (オフライン時はアプリシェルのみ)
        runtimeCaching: [
          {
            urlPattern: ({ url }) => url.pathname.startsWith("/admin") || url.pathname.startsWith("/graphql"),
            handler: "NetworkOnly"
          }
        ]
      }
    })
  ],
  clearScreen: false,
  server: { port: 5173, strictPort: true },
  build: { outDir: "dist", target: "esnext", sourcemap: false },
});
