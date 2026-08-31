import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import { fileURLToPath, URL } from "node:url";

// Tauri injecte cette variable lorsqu'on développe depuis un appareil distant.
const host = process.env.TAURI_DEV_HOST;

export default defineConfig({
  plugins: [react(), tailwindcss()],

  // Laisse les erreurs de compilation Rust visibles dans le terminal.
  clearScreen: false,

  server: {
    port: 1420,
    // Port figé : Tauri ne sait pas suivre un port qui change au démarrage.
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: "ws", host, port: 1421 } : undefined,
    watch: {
      // Le backend Rust a son propre rechargement à chaud : Vite doit l'ignorer.
      ignored: ["**/src-tauri/**"],
    },
  },

  resolve: {
    alias: {
      "@": fileURLToPath(new URL("./src", import.meta.url)),
    },
  },

  build: {
    // Safari 13+ correspond au WebKit embarqué par macOS.
    target: "safari15",
    minify: process.env.TAURI_ENV_DEBUG ? false : "esbuild",
    sourcemap: !!process.env.TAURI_ENV_DEBUG,
  },
});
