import { defineConfig, mergeConfig } from "vite";

import app from "./vite.config";

export default mergeConfig(
  app,
  defineConfig({
    build: {
      outDir: "../demo-dist",
      rolldownOptions: { input: new URL("web/demo/index.html", import.meta.url).pathname },
    },
  }),
);
