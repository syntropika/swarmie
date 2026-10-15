import { defineConfig, mergeConfig } from "vite";

import app from "./vite.config";

export default mergeConfig(
  app,
  defineConfig({
    root: "src",
    build: {
      outDir: "../demo-dist",
      rolldownOptions: { input: new URL("src/demo/index.html", import.meta.url).pathname },
    },
  }),
);
