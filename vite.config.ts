import { sveltekit } from "@sveltejs/kit/vite";
import process from "node:process";
import { defineConfig } from "vitest/config";

const host = process.env.TAURI_DEV_HOST;

// https://v2.tauri.app/start/frontend/sveltekit/
export default defineConfig({
  plugins: [sveltekit()],
  // Don't hide Rust error messages
  clearScreen: false,
  server: {
    // Tauri expects a fixed port
    port: 1420,
    strictPort: true,
    host: host || "127.0.0.1",
    hmr: host ? { protocol: "ws", host, port: 1421 } : undefined,
    watch: { ignored: ["**/src-tauri/**", "**/crates/**", "**/target/**"] },
  },
  test: {
    include: ["src/**/*.test.ts"],
    environment: "node",
  },
});
