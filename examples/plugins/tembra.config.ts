import { defineConfig } from "@tembra/sdk/tooling";

const watch = [
  "Cargo.toml",
  "Cargo.lock",
  "crates/plugin-contracts/Cargo.toml",
  ...new Bun.Glob("crates/plugin-contracts/src/**/*.rs").scanSync(),
];
const contracts = {
  counter: "Counter",
  "remote-counter": "RemoteCounter",
  "dynamic-consumer": "DynamicConsumer",
  "dynamic-consumer-v2": "DynamicConsumerV2",
  "plugin-configuration": "PluginConfiguration",
  "remote-browser-access": "RemoteBrowserAccess",
};

export default defineConfig({
  root: "../..",
  outDir: "target/plugins",
  tsconfig: "examples/plugins/tsconfig.json",
  codegen: Object.entries(contracts).map(([source, name]) => ({
    source: {
      command: [
        "cargo",
        "+1.99.0",
        "run",
        "--locked",
        "--quiet",
        "-p",
        "swarmie-plugin-contracts",
        "--bin",
        "export-contract",
        "--",
        source,
      ],
      watch,
    },
    typescript: `target/generated/contracts/${source}.ts`,
    name,
  })),
  builds: {
    "partial-install": {
      entry: "tests/plugins/fixtures/partial-install.ts",
      platform: "server",
      export: "first",
    },
    "alternative-counter": {
      entry: "examples/plugins/alternative-counter/index.ts",
      platform: "server",
    },
    "dynamic-consumer-v1": {
      entry: "examples/plugins/dynamic-consumer/index.ts",
      platform: "server",
    },
    "dynamic-consumer-v2": { entry: "examples/plugins/dynamic-consumer/v2.ts", platform: "server" },
    "dynamic-consumer-failing": {
      entry: "tests/plugins/fixtures/dynamic-consumer/failing.ts",
      platform: "server",
    },
    "dynamic-consumer-provider-removed": {
      entry: "tests/plugins/fixtures/dynamic-consumer/provider-removed.ts",
      platform: "server",
    },
    "dynamic-consumer-incompatible": {
      entry: "tests/plugins/fixtures/dynamic-consumer/incompatible.ts",
      platform: "server",
    },
    "dynamic-consumer-evaluation-failure": {
      entry: "tests/plugins/fixtures/dynamic-consumer/evaluation-failure.ts",
      platform: "server",
    },
    "dynamic-consumer-invalid-result": {
      entry: "tests/plugins/fixtures/dynamic-consumer/invalid-result.ts",
      platform: "server",
    },
    "dynamic-consumer-invalid-argument": {
      entry: "tests/plugins/fixtures/dynamic-consumer/invalid-argument.ts",
      platform: "server",
    },
    "browser-widget": { entry: "tests/plugins/fixtures/browser-metadata.ts", platform: "browser" },
  },
});
