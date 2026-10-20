import { defineConfig } from "@tembra/sdk/tooling";

const contracts = [
  ["counter", "Counter", "swarmie-counter", "fixture-contract"],
  ["remote-counter", "RemoteCounter", "swarmie-counter", "fixture-contract"],
  ["dynamic-consumer", "DynamicConsumer", "swarmie-counter", "fixture-contract"],
  ["dynamic-consumer-v2", "DynamicConsumerV2", "swarmie-counter", "fixture-contract"],
  ["configuration-control", "ConfigurationControl", "swarmie-counter", "fixture-contract"],
  ["plugin-configuration", "PluginConfiguration", "swarmie-plugin-contracts", "export-contract"],
  ["remote-browser-access", "RemoteBrowserAccess", "swarmie-plugin-contracts", "export-contract"],
  ["memory-store", "MemoryStore", "swarmie-memory-store", "memory-store-contract"],
] as const;
const watch = [
  "Cargo.toml",
  "Cargo.lock",
  ...["crates/plugin-contracts", "tests/plugins/fixtures/counter", "plugins/memory-store"].flatMap(
    (owner) => [`${owner}/Cargo.toml`, ...new Bun.Glob(`${owner}/src/**/*.rs`).scanSync()],
  ),
];

export default defineConfig({
  root: "../..",
  outDir: "target/plugins",
  tsconfig: "tests/plugins/tsconfig.json",
  codegen: contracts.map(([source, name, owner, binary]) => ({
    source: {
      command: [
        "cargo",
        "+1.99.0",
        "run",
        "--locked",
        "--quiet",
        "-p",
        owner,
        "--bin",
        binary,
        ...(source === "memory-store" ? [] : ["--", source]),
      ],
      watch,
    },
    typescript: `target/generated/contracts/${source}.ts`,
    name,
  })),
  builds: {
    "dynamic-memory-store-apply-failing": {
      entry: "tests/plugins/fixtures/dynamic-memory-store/apply-failing.ts",
      platform: "server",
    },
    "dynamic-memory-store": {
      entry: "tests/plugins/fixtures/dynamic-memory-store/index.ts",
      platform: "server",
    },
    "partial-install": {
      entry: "tests/plugins/fixtures/partial-install.ts",
      platform: "server",
      export: "first",
    },
    "alternative-counter": {
      entry: "tests/plugins/fixtures/alternative-counter/index.ts",
      platform: "server",
    },
    "dynamic-consumer-v1": {
      entry: "tests/plugins/fixtures/dynamic-consumer/index.ts",
      platform: "server",
    },
    "dynamic-consumer-v2": {
      entry: "tests/plugins/fixtures/dynamic-consumer/v2.ts",
      platform: "server",
    },
    "dynamic-consumer-failing": {
      entry: "tests/plugins/fixtures/dynamic-consumer/failing.ts",
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
