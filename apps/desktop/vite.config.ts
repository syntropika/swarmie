import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

export default defineConfig({
  root: "web",
  plugins: [react(), tailwindcss()],
  resolve: { alias: { "@": new URL("./web/src", import.meta.url).pathname } },
  build: { rolldownOptions: { input: { app: new URL("./web/index.html", import.meta.url).pathname, contributions: new URL("./web/contributions-demo.html", import.meta.url).pathname } }, outDir: "../dist", emptyOutDir: true, target: "es2022" },
  server: {
    host: "127.0.0.1",
    proxy: Object.fromEntries(["/rpc", "/client/rpc", "/nodes", "/preview"].map((path) => [path, {
      target: "http://127.0.0.1:7447",
      changeOrigin: true,
      headers: { Origin: "http://127.0.0.1:7447" },
    }])),
  },
});
