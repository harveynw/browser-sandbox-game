import { defineConfig } from "vite";

const proxy = {
  "/api": { target: "http://127.0.0.1:3001", changeOrigin: true },
  "/health": { target: "http://127.0.0.1:3001", changeOrigin: true },
};

export default defineConfig({
  server: {
    host: "localhost",
    port: 5173,
    strictPort: true,
    proxy,
  },
  preview: {
    host: "localhost",
    port: 4173,
    strictPort: true,
    proxy,
  },
});
