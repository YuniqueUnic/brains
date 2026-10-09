import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

// Dev proxy mirrors the same-origin deployment: /api/v1 reaches brains-server.
// VITE_BASE supports project-pages deployments (e.g. "/brains/").
export default defineConfig({
  base: process.env.VITE_BASE ?? "/",
  plugins: [react(), tailwindcss()],
  build: {
    // Editor and tree dependencies cache independently from the app shell.
    rollupOptions: {
      output: {
        manualChunks(id) {
          if (id.includes("/node_modules/@codemirror/") || id.includes("/node_modules/@lezer/")) return "editor";
          if (id.includes("/node_modules/react-aria") || id.includes("/node_modules/@react-aria/") || id.includes("/node_modules/@react-stately/")) return "file-tree";
        },
      },
    },
    chunkSizeWarningLimit: 1100,
  },
  server: {
    proxy: {
      "/api": "http://127.0.0.1:45800",
    },
  },
});
