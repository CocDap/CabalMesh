import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { resolve } from "node:path";

// @ts-expect-error process is a nodejs global
const host = process.env.TAURI_DEV_HOST;

export default defineConfig({
  plugins: [react()],

  root: resolve(__dirname, "src/mobile-entry"),

  build: {
    outDir: resolve(__dirname, "dist"),
    emptyOutDir: true,
  },

  // Prevent Vite from obscuring rust errors.
  clearScreen: false,

  server: {
    // @ts-expect-error process is a nodejs global
    port: parseInt(process.env.PORT || "1420"),
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          // A distinct port from the dev server. Sharing one meant HMR never
          // attached on a physical device.
          port: 1421,
        }
      : undefined,
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
});
