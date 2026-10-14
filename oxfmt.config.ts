import { defineConfig } from "oxfmt";
import ultracite from "ultracite/oxfmt";

export default defineConfig({
  ...ultracite,
  printWidth: 100,
  trailingComma: "all",
  ignorePatterns: [
    ...ultracite.ignorePatterns,
    "**/target/**",
    "docs/**",
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
    stylesheet: "./packages/ui/src/styles.css",
  },
});
