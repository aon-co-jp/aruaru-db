import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { VitePWA } from "vite-plugin-pwa";

export default defineConfig({
  plugins: [
    react(),
    VitePWA({
      registerType: "autoUpdate",
      manifest: false,          // public/manifest.webmanifest を使用
      manifestFilename: "manifest.webmanifest",
      includeManifestIcons: false,
      workbox: {
        globPatterns: ["**/*.{js,css,html,ico,png,svg,webmanifest}"],
        runtimeCaching: [
          {
            // GraphQL は常にネットワーク優先（キャッシュしない）
            urlPattern: /\/graphql$/,
            handler: "NetworkOnly",
          },
        ],
      },
    }),
  ],
  server: {
    port: 5174,
    cors: true,
  },
  build: {
    outDir: "dist",
    sourcemap: true,
  },
});
