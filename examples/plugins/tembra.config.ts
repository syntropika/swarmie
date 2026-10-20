export default {
  root: "../..",
  outDir: "target/plugins",
  tsconfig: "examples/plugins/tsconfig.json",
  builds: {
    "dynamic-consumer-v1": { entry: "examples/plugins/dynamic-consumer-v1.ts", platform: "server" },
    "dynamic-consumer-v2": { entry: "examples/plugins/dynamic-consumer-v2.ts", platform: "server" },
    "dynamic-consumer-failing": {
      entry: "examples/plugins/dynamic-consumer-failing.ts",
      platform: "server",
    },
    "dynamic-consumer-provider-removed": {
      entry: "examples/plugins/dynamic-consumer-provider-removed.ts",
      platform: "server",
    },
    "dynamic-consumer-incompatible": {
      entry: "examples/plugins/dynamic-consumer-incompatible.ts",
      platform: "server",
    },
    "dynamic-consumer-evaluation-failure": {
      entry: "examples/plugins/dynamic-consumer-evaluation-failure.ts",
      platform: "server",
    },
    "dynamic-consumer-invalid-result": {
      entry: "examples/plugins/dynamic-consumer-invalid-result.ts",
      platform: "server",
    },
    "dynamic-consumer-invalid-argument": {
      entry: "examples/plugins/dynamic-consumer-invalid-argument.ts",
      platform: "server",
    },
    "browser-widget": { entry: "examples/plugins/browser-widget.ts", platform: "browser" },
  },
};
