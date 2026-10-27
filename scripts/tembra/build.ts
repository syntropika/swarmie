import { build, Platform } from "@syntropika/tembra-sdk/tooling";

// The current foundation uses native declarations; no legacy contract generator runs here.
const artifacts = await build({
  root: process.cwd(),
  outDir: "target/plugins",
  builds: {
    "counter-browser": {
      entry: "tests/fixtures/tembra-sdk/counter.ts",
      platform: Platform.Browser,
    },
    "counter-embedded": {
      entry: "tests/fixtures/tembra-sdk/counter.ts",
      platform: Platform.Embedded,
    },
  },
});
console.log(`Built ${artifacts.length} orb artifacts`);
