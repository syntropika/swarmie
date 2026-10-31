import { createHash } from "node:crypto";
import { createRequire } from "node:module";
import nodePath from "node:path";
import { fileURLToPath } from "node:url";

import { THEME_PREFERENCE_BOOTSTRAP } from "@swarmie/ui-web/theme";
import tailwind from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig, searchForWorkspaceRoot } from "vite";

const sdk = fileURLToPath(import.meta.resolve("@syntropika/tembra-sdk/browser"));
const sdkRequire = createRequire(sdk);
const wasm = sdkRequire.resolve("@syntropika/tembra-engine-wasm/tembra_bg.wasm");
export default defineConfig(({ command }) => ({
  plugins: [
    ...(command === "build"
      ? [
          {
            name: "swarmie-bundled-content-policy",
            transformIndexHtml: {
              order: "post",
              handler: (html: string) => {
                const hashes = [
                  ...html.matchAll(/<script\b(?![^>]*\bsrc=)[^>]*>(?<source>[\s\S]*?)<\/script>/gu),
                ].map(
                  (match) =>
                    `'sha256-${createHash("sha256")
                      .update(match.groups?.source ?? "")
                      .digest("base64")}'`,
                );
                const policy = [
                  "default-src 'self'",
                  `script-src 'self' 'wasm-unsafe-eval' ${hashes.join(" ")}`,
                  "style-src 'self' 'unsafe-inline'",
                  "img-src 'self' data: blob: https:",
                  "font-src 'self' data:",
                  "connect-src 'self' https: wss: http://127.0.0.1:* ws://127.0.0.1:*",
                  "worker-src 'self' blob:",
                  "object-src 'none'",
                  "base-uri 'none'",
                  "frame-src 'none'",
                  "form-action 'none'",
                  "require-trusted-types-for 'script'",
                ].join("; ");
                return {
                  html,
                  tags: [
                    {
                      tag: "meta",
                      attrs: { "http-equiv": "Content-Security-Policy", content: policy },
                      injectTo: "head-prepend",
                    },
                  ],
                };
              },
            },
          },
        ]
      : []),
    react(),
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
    watch: { ignored: ["**/target/**"] },
    fs: {
      allow: [searchForWorkspaceRoot(process.cwd()), nodePath.dirname(sdk), nodePath.dirname(wasm)],
    },
  },
  build: { outDir: "dist", target: "esnext" },
}));
