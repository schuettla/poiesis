import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";

// @tauri-apps/cli sets TAURI_DEV_HOST when running `tauri android dev` against a phone.
const host = process.env.TAURI_DEV_HOST;

export default defineConfig({
  plugins: [react()],
  // The phone app is served from a folder inside the app, never from `/`.
  base: "./",
  clearScreen: false,
  server: {
    port: 1430,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: "ws", host, port: 1431 } : undefined,
    watch: { ignored: ["**/src-tauri/**"] },
  },
  test: { environment: "jsdom", include: ["src/**/*.test.{ts,tsx}"] },
});
