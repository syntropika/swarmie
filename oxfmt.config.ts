import { defineConfig } from "oxfmt";
import ultracite from "ultracite/oxfmt";

export default defineConfig({
  ...ultracite,
  printWidth: 100,
  trailingComma: "all",
  ignorePatterns: [
    ...ultracite.ignorePatterns,
    "**/target/**",
    // Preserve the reproducible third-party dependency snapshot and its patch digest.
    "vendor/genai-0.7.0/**",
    "packages/ui/web/vendor/**",
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
