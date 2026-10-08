import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

// Dev proxy mirrors the same-origin deployment: /api/v1 reaches brain-server.
export default defineConfig({
  plugins: [react(), tailwindcss()],
  build: {
    // CodeMirror + marked account for the bundle; this is a local-first app.
    chunkSizeWarningLimit: 900,
  },
  server: {
    proxy: {
      "/api": "http://127.0.0.1:8787",
    },
  },
});
