import { defineConfig } from "oxlint";
import antiSlop from "ultracite/oxlint/anti-slop";
import core from "ultracite/oxlint/core";
import react from "ultracite/oxlint/react";
import shadcn from "ultracite/oxlint/shadcn";
import tanstack from "ultracite/oxlint/tanstack";

const frontendSources = [
  "packages/ui/{web,native}/src/**/*.{ts,tsx}",
  "apps/desktop/src/**/*.{ts,tsx}",
  "apps/web/src/**/*.{ts,tsx}",
  "packages/auth/src/**/*.{ts,tsx}",
];

const forbiddenStyleProps = ["style", "css"].map((propName) => ({
  propName,
  message:
    "Use Tailwind className utilities. Shared tokens belong in packages/ui/web/src/styles.css.",
}));
const stylingImportPatterns = [
  {
    group: [
      ...["css", "scss", "sass", "less", "styl", "stylus", "pcss", "postcss"].flatMap(
        (extension) => [`*.${extension}`, `*.${extension}[?#]*`],
      ),
      "@swarmie/config/fonts",
      "@swarmie/config/fonts[?#]*",
      ...[
        "@emotion",
        "@linaria",
        "@pandacss",
        "@stitches",
        "@vanilla-extract",
        "aphrodite",
        "goober",
        "jss",
        "react-jss",
        "styled-components",
        "styled-jsx",
        "stylex",
        "@stylexjs",
        "twin.macro",
      ].flatMap((library) => [library, `${library}/*`, `${library}[?#]*`]),
    ],
    message:
      "Use Tailwind and @swarmie/ui instead of component CSS or CSS-in-JS. Only frontend roots load their owned stylesheets.",
  },
];

export default defineConfig({
  extends: [core, react, tanstack],
  jsPlugins: [...antiSlop.jsPlugins, ...shadcn.jsPlugins, "eslint-plugin-better-tailwindcss"],
  ignorePatterns: [
    ...core.ignorePatterns,
    "**/target/**",
    "**/.tanstack/**",
    "**/.expo/**",
    "**/.wrangler/**",
    "**/.nitro/**",
    "docs/**",
    // Installed upstream skills retain their own source conventions.
    ".agents/skills/heroui-native/**",
    ".agents/skills/heroui-react/**",
    // The hosted repository owns its own root configuration and dependency graph.
    "cloud/**",
    "tests/fixtures/**",
    "**/storybook-static/**",
    "**/dist/**",
    "packages/contracts/*.d.ts",
  ],
  options: { typeAware: true },
  settings: {
    shadcn: {
      ui: "@swarmie/ui-web",
      note: "Use shared Swarmie components, semantic theme tokens, and Tailwind utilities. Use the shared component contracts.",
    },
    "better-tailwindcss": {
      entryPoint: "./packages/ui/web/src/styles.css",
    },
  },
  rules: {
    // Function declarations retain useful hoisting; either function syntax is valid.
    "func-style": "off",
    "no-use-before-define": ["error", { functions: false }],
    // Object insertion order can be part of a contract; formatting owns import order.
    "sort-keys": "off",
    // Explicit undefined preserves value-returning callbacks and exhaustive optional returns.
    "unicorn/no-useless-undefined": "off",
    "no-unused-vars": ["error", { ignoreRestSiblings: true }],
    // The typed rule understands returned thenables; the syntax-only rule does not.
    "require-await": "off",
    "typescript/require-await": "error",
    // Callback names describe capability actions; on* props already identify handlers.
    "react/jsx-handler-names": "off",
    // Compiler implementation TODOs are not runtime correctness failures.
    "react/todo": "off",
    // Synchronous event boundaries may attach rejection handlers without becoming async.
    "promise/prefer-await-to-then": "off",
    "typescript/parameter-properties": [
      "error",
      { allow: ["readonly", "private readonly", "protected readonly", "public readonly"] },
    ],
    "typescript/consistent-type-definitions": ["error", "type"],
    "react/function-component-definition": [
      "error",
      {
        namedComponents: ["function-declaration", "arrow-function"],
        unnamedComponents: "arrow-function",
      },
    ],
    "typescript/no-floating-promises": ["error", { ignoreVoid: false }],
    "typescript/ban-ts-comment": [
      "error",
      { "ts-ignore": true, "ts-nocheck": true, "ts-expect-error": "allow-with-description" },
    ],
    "react/no-array-index-key": "error",
    "anti-slop/no-chained-type-assertions": "error",
    "anti-slop/no-known-value-widening": "error",
    "anti-slop/no-widen-then-assert": "error",
    "anti-slop/no-module-mocking": "error",
    "anti-slop/require-safety-comment-for-type-assertion": "error",
    "react/forbid-dom-props": ["error", { forbid: forbiddenStyleProps }],
    "react/forbid-component-props": ["error", { forbid: forbiddenStyleProps }],
    "react/forbid-elements": [
      "error",
      {
        forbid: [
          {
            element: "style",
            message:
              "Use Tailwind utilities; shared tokens, fonts, and keyframes belong in owned stylesheets.",
          },
        ],
      },
    ],
    "no-restricted-imports": ["error", { patterns: stylingImportPatterns }],
  },
  overrides: [
    {
      // The native Router root owns its single Tailwind stylesheet.
      files: ["apps/mobile/src/app/_layout.tsx"],
      rules: {
        "no-restricted-imports": [
          "error",
          {
            patterns: stylingImportPatterns.map((pattern) => ({
              ...pattern,
              group: [...pattern.group, "!~/src/global.css"],
            })),
          },
        ],
      },
    },
    {
      files: ["apps/desktop/src/main.tsx", "apps/desktop/src/extension-panel.tsx"],
      rules: {
        "no-restricted-imports": [
          "error",
          {
            patterns: stylingImportPatterns.map((pattern) => ({
              ...pattern,
              group: [...pattern.group, "!~/src/styles.css", "!@xterm/xterm/css/xterm.css"],
            })),
          },
        ],
      },
    },
    {
      // Application entry points own the stylesheets loaded once for their frontend.
      files: ["apps/docs/src/app/layout.tsx"],
      rules: {
        "no-restricted-imports": [
          "error",
          {
            patterns: stylingImportPatterns.map((pattern) => ({
              ...pattern,
              group: [...pattern.group, "!~/src/app/global.css"],
            })),
          },
        ],
      },
    },
    {
      // The isolated desktop demo is also a frontend root, not a component stylesheet.
      files: ["apps/desktop/src/demo/main.tsx"],
      rules: {
        "no-restricted-imports": [
          "error",
          {
            patterns: stylingImportPatterns.map((pattern) => ({
              ...pattern,
              group: [...pattern.group, "!~/src/styles.css", "!~/src/demo/styles.css"],
            })),
          },
        ],
      },
    },
    ...core.overrides,
    ...tanstack.overrides,
    {
      // JavaScript tooling has no checked type contracts. Native JS rules still apply;
      // TypeScript source and tests receive the complete type-aware policy.
      files: ["**/*.{js,mjs,cjs}"],
      rules: Object.fromEntries(
        Object.keys(core.rules)
          .filter((rule) => rule.startsWith("typescript/"))
          .map((rule) => [rule, "off" as const]),
      ),
    },
    {
      files: ["**/*.{test,spec}.{ts,tsx,js,mjs}"],
      // Async fixture adapters preserve rejection semantics without requiring fake awaits.
      rules: { "typescript/require-await": "off" },
    },
    {
      files: frontendSources,
      rules: {
        ...shadcn.rules,
        "shadcn/no-arbitrary-values": [
          "error",
          { allow: ["layout", "[-webkit-tap-highlight-color:transparent]"] },
        ],
        "better-tailwindcss/no-concatenated-classes": "error",
        "better-tailwindcss/no-conflicting-classes": "error",
      },
    },
    {
      // Primitive implementations own variants and compose class factories themselves.
      files: ["packages/ui/{web,native}/src/**/*.{ts,tsx}"],
      rules: { "shadcn/no-restyle": "off", "shadcn/require-static-classes": "off" },
    },
    {
      files: [
        "packages/contracts/**/*.{ts,tsx}",
        "sdk/typescript/src/**/*.{ts,tsx}",
        "packages/api/src/**/*.{ts,tsx}",
      ],
      rules: {
        "no-restricted-imports": [
          "error",
          {
            patterns: [
              ...stylingImportPatterns,
              {
                group: [
                  "react",
                  "react-dom",
                  "react-dom/*",
                  "@swarmie/ui-web",
                  "@swarmie/ui-native",
                  "@swarmie/auth",
                  "@swarmie/web",
                  "swarmie-desktop",
                ],
                message:
                  "Shared HTTP contracts must remain React-free. Compose UI and workflows in the consuming capability.",
              },
            ],
          },
        ],
      },
    },
    {
      files: ["packages/ui/{web,native}/src/**/*.{ts,tsx}"],
      rules: {
        "no-restricted-imports": [
          "error",
          {
            patterns: [
              ...stylingImportPatterns,
              {
                group: [
                  "@swarmie/auth",
                  "@swarmie/web",
                  "swarmie-desktop",
                  "swarmie-desktop/*",
                  "@swarmie/api",
                  "@swarmie/api/*",
                  "@swarmie/auth",
                  "@swarmie/auth/*",
                  "@swarmie/web",
                  "@swarmie/web/*",
                  "@swarmie/client",
                  "@swarmie/client/*",
                  "@tanstack/react-router",
                  "@tanstack/react-query",
                  "@tanstack/react-start",
                ],
                message:
                  "Shared UI receives data, labels, and callbacks from capabilities; do not import transport, routing, or application state.",
              },
            ],
          },
        ],
      },
    },
    {
      // TanStack Router uses Fetch Responses for redirects and route control flow.
      files: ["apps/web/src/**/*.{ts,tsx}", "packages/auth/src/**/*.{ts,tsx}"],
      rules: {
        "typescript/only-throw-error": ["error", { allow: [{ from: "lib", name: "Response" }] }],
      },
    },
    {
      // The website document entry owns its single stylesheet.
      files: ["apps/web/src/routes/__root.tsx"],
      rules: {
        "no-restricted-imports": [
          "error",
          {
            patterns: stylingImportPatterns.map((pattern) => ({
              ...pattern,
              group: [...pattern.group, "!~/src/styles.css[?]url"],
            })),
          },
        ],
      },
    },
    {
      files: ["packages/ui/*/.storybook/preview.tsx"],
      rules: {
        "no-restricted-imports": [
          "error",
          {
            patterns: stylingImportPatterns.map((pattern) => ({
              ...pattern,
              group: [...pattern.group, "!../src/styles.css"],
            })),
          },
        ],
      },
    },
  ],
});
