import { fileURLToPath, URL } from "node:url";
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import { stepBindingsPlugin } from "./scripts/step-bindings.mjs";

// Tauri expects a fixed port and must not have the console cleared under it.
export default defineConfig({
  plugins: [stepBindingsPlugin(), react(), tailwindcss()],
  clearScreen: false,
  resolve: {
    alias: { "@": fileURLToPath(new URL("./src", import.meta.url)) },
  },
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ["**/src-tauri/**"] },
  },
  build: {
    target: "es2022",
    // Bundles load from disk inside the desktop app, so chunk size matters little.
    chunkSizeWarningLimit: 4000,
  },
});
