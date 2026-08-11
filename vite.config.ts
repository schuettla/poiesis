import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";

// @tauri-apps/cli sets TAURI_DEV_HOST when running `tauri dev` on a device/host.
const host = process.env.TAURI_DEV_HOST;

// https://vitejs.dev/config/
export default defineConfig(async () => ({
  plugins: [react()],

  // Tauri expects a fixed port and fails if it is not available.
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // Don't watch the Rust backend; it has its own watcher via `tauri dev`.
      ignored: ["**/src-tauri/**"],
    },
  },

  // Produce a build that Tauri can package.
  build: {
    target: "chrome105",
    minify: !process.env.TAURI_DEBUG ? "esbuild" : false,
    sourcemap: !!process.env.TAURI_DEBUG,
  },

  // Unit tests over lib/ logic default to `node` — the frontend is designed to
  // import cleanly outside the desktop app (see `inTauri()`). The render smoke
  // test opts into jsdom per-file with a `@vitest-environment` docblock.
  test: {
    environment: "node",
    include: ["src/**/*.test.{ts,tsx}"],
  },
}));
