import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

export default defineConfig({
  root: "web",
  plugins: [react(), tailwindcss()],
  resolve: { alias: { "@": new URL("web/src", import.meta.url).pathname } },
  build: { outDir: "../dist", emptyOutDir: true, target: "es2022" },
  server: {
    host: "127.0.0.1",
    proxy: Object.fromEntries(
      ["/rpc", "/client/rpc", "/nodes", "/preview"].map((path) => [
        path,
        {
          target: "http://127.0.0.1:7447",
          changeOrigin: true,
          headers: { Origin: "http://127.0.0.1:7447" },
        },
      ]),
    ),
  },
});
