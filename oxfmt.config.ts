import { defineConfig } from "oxfmt";
import ultracite from "ultracite/oxfmt";

export default defineConfig({
  ...ultracite,
  printWidth: 100,
  trailingComma: "all",
  ignorePatterns: [
    ...ultracite.ignorePatterns,
    "**/target/**",
    "packages/tembra-sdk/upstream/**",
    // Generated verbatim from canonical Tembra descriptors; verified by tembra-contracts-check.
    "examples/plugins/generated/**",
    ".agents/skills/heroui-native/**",
    ".agents/skills/heroui-react/**",
    "tests/fixtures/**",
    "**/storybook-static/**",
    "**/dist/**",
    "**/*.lock",
    "**/package-lock.json",
    "**/.tanstack/**",
    "**/.nitro/**",
    "**/*.md",
    "**/*.toml",
  ],
  sortTailwindcss: {
    functions: ["clsx", "cva", "tw", "twMerge", "cn", "twJoin", "tv"],
    stylesheet: "./packages/config/tailwind/tooling-tokens.css",
  },
});
