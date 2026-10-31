import { defineConfig } from "oxlint";
import antiSlop from "ultracite/oxlint/anti-slop";
import core from "ultracite/oxlint/core";
import react from "ultracite/oxlint/react";
import shadcn from "ultracite/oxlint/shadcn";
import tanstack from "ultracite/oxlint/tanstack";

const frontendSources = [
  "packages/ui/{web,native}/src/**/*.{ts,tsx}",
  "apps/web/src/**/*.{ts,tsx}",
  "apps/web-client/src/**/*.{ts,tsx}",
  "plugins/*/browser/src/**/*.{ts,tsx}",
  "plugins/browser/src/**/*.{ts,tsx}",
  "packages/client-ui/src/**/*.{ts,tsx}",
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
      "Use Tailwind and the shared @swarmie/ui-web or @swarmie/ui-native package instead of component CSS or CSS-in-JS. Only frontend roots load their owned stylesheets.",
  },
];

export default defineConfig({
  extends: [core, react, tanstack],
  jsPlugins: [...antiSlop.jsPlugins, ...shadcn.jsPlugins, "eslint-plugin-better-tailwindcss"],
  ignorePatterns: [
    ...core.ignorePatterns,
    "**/target/**",
    "packages/ui/web/vendor/**",
    "**/.tanstack/**",
    "**/.expo/**",
    "**/.wrangler/**",
    "**/.nitro/**",
    // Installed upstream skills retain their own source conventions.
    ".agents/skills/heroui-native/**",
    ".agents/skills/heroui-react/**",
    "tests/fixtures/**",
    "**/storybook-static/**",
    "**/dist/**",
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
      // The client document entry owns its stylesheet.
      files: ["apps/web-client/src/main.tsx"],
      rules: {
        "no-restricted-imports": [
          "error",
          {
            patterns: stylingImportPatterns.map((pattern) => ({
              ...pattern,
              group: [...pattern.group, "!./styles.css"],
            })),
          },
        ],
      },
    },
    {
      // The native Router root owns its single Tailwind stylesheet.
      files: ["apps/mobile-client/src/app/_layout.tsx"],
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
      files: ["packages/ui/{web,native}/src/**/*.{ts,tsx}"],
      rules: {
        "no-restricted-imports": [
          "error",
          {
            patterns: [
              ...stylingImportPatterns,
              {
                group: [
                  "@swarmie/web",
                  "swarmie-desktop",
                  "swarmie-desktop/*",
                  "@swarmie/web/*",
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
      files: ["apps/web/src/**/*.{ts,tsx}"],
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
      // Catalog preview roots own their global stylesheet entries.
      files: ["packages/ui/*/.storybook/preview.tsx"],
      rules: {
        "no-restricted-imports": [
          "error",
          {
            patterns: stylingImportPatterns.map((pattern) => ({
              ...pattern,
              group: [...pattern.group, "!../src/styles.css", "!./preview.css"],
            })),
          },
        ],
      },
    },
  ],
});
