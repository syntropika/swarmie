import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

export default defineConfig({
  root: "web",
  plugins: [react(), tailwindcss()],
  resolve: { alias: { "@": new URL("./web/src", import.meta.url).pathname } },
  build: { outDir: "../dist", emptyOutDir: true, target: "es2022" },
  server: {
    host: "127.0.0.1",
    proxy: {
      "/rpc": {
        target: "http://127.0.0.1:7447",
        changeOrigin: true,
        headers: { Origin: "http://127.0.0.1:7447" },
      },
    },
  },
});
