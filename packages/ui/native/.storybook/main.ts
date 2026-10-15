import { existsSync } from "node:fs";
import { createRequire } from "node:module";
import path from "node:path";

import { transformSync } from "@babel/core";
import type { StorybookConfig } from "@storybook/react-vite";
import tailwindcss from "@tailwindcss/vite";
import { mergeConfig } from "vite";

const workletsPlugin = createRequire(import.meta.url).resolve("react-native-worklets/plugin");

const config: StorybookConfig = {
  stories: ["../src/**/*.stories.tsx"],
  addons: ["@storybook/addon-docs", "@storybook/addon-a11y"],
  framework: "@storybook/react-vite",
  core: { disableTelemetry: true },
  // Native props are checked by TypeScript; web docgen cannot parse native Flow dependencies.
  typescript: { reactDocgen: false },
  viteFinal: (viteConfig) =>
    mergeConfig(viteConfig, {
      define: { __DEV__: "true" },
      resolve: {
        alias: [{ find: /^react-native$/u, replacement: "react-native-web" }],
        extensions: [
          ".web.mjs",
          ".web.js",
          ".web.jsx",
          ".web.ts",
          ".web.tsx",
          ".mjs",
          ".js",
          ".mts",
          ".ts",
          ".jsx",
          ".tsx",
          ".json",
        ],
      },
      plugins: [
        tailwindcss(),
        {
          name: "native-design-system-stories",
          enforce: "pre",
          // Native libraries ship explicit .js imports; choose their web implementations for this catalog.
          resolveId(source: string, importer: string | undefined) {
            if (importer !== undefined && source.startsWith(".") && source.endsWith(".js")) {
              const web = path.resolve(path.dirname(importer), source.replace(/\.js$/u, ".web.js"));
              if (existsSync(web)) {
                return web;
              }
            }
            return undefined;
          },
          // Metro performs this rewrite in native clients; Storybook uses the web Uniwind components.
          transform(source: string, id: string) {
            if (id.endsWith("/native/src/controls.tsx") || id.includes("/heroui-native/")) {
              const rewritten = source
                .replaceAll('from "react-native"', 'from "uniwind/components"')
                .replaceAll("from 'react-native'", "from 'uniwind/components'");
              // Vite does not run Expo's Babel preset; vendor animations still need worklet metadata.
              if (id.includes("/heroui-native/") && id.endsWith(".js")) {
                return transformSync(rewritten, {
                  filename: id,
                  babelrc: false,
                  configFile: false,
                  sourceMaps: true,
                  plugins: [workletsPlugin],
                });
              }
              return rewritten;
            }
            return undefined;
          },
        },
      ],
    }),
};

export default config;
