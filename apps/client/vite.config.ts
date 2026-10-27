import { createRequire } from "node:module";
import nodePath from "node:path";
import { fileURLToPath } from "node:url";

import { THEME_PREFERENCE_BOOTSTRAP } from "@swarmie/ui-web/theme";
import tailwind from "@tailwindcss/vite";
import { defineConfig, searchForWorkspaceRoot } from "vite";

const sdk = fileURLToPath(import.meta.resolve("@syntropika/tembra-sdk/browser"));
const sdkRequire = createRequire(sdk);
const wasm = sdkRequire.resolve("@syntropika/tembra-engine-wasm/tembra_bg.wasm");
export default defineConfig({
  plugins: [
    tailwind(),
    {
      name: "swarmie-theme",
      transformIndexHtml: (html) => ({
        html,
        tags: [{ tag: "script", children: THEME_PREFERENCE_BOOTSTRAP, injectTo: "head" }],
      }),
    },
  ],
  resolve: { dedupe: ["react", "react-dom"], tsconfigPaths: true },
  server: {
    fs: {
      allow: [searchForWorkspaceRoot(process.cwd()), nodePath.dirname(sdk), nodePath.dirname(wasm)],
    },
  },
  build: { outDir: "dist", target: "esnext" },
});
