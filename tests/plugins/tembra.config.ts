import { defineConfig } from "@tembra/sdk/tooling";

const contracts = [
  ["session-store", "SessionStore", "swarmie-session-contracts", "session-contract"],
  ["counter", "Counter", "swarmie-counter", "fixture-contract"],
  ["remote-counter", "RemoteCounter", "swarmie-counter", "fixture-contract"],
  ["dynamic-consumer", "DynamicConsumer", "swarmie-counter", "fixture-contract"],
  ["dynamic-consumer-v2", "DynamicConsumerV2", "swarmie-counter", "fixture-contract"],
  ["configuration-control", "ConfigurationControl", "swarmie-counter", "fixture-contract"],
  ["plugin-configuration", "PluginConfiguration", "swarmie-plugin-contracts", "export-contract"],
  ["remote-browser-access", "RemoteBrowserAccess", "swarmie-plugin-contracts", "export-contract"],
  ["memory-store", "MemoryStore", "swarmie-memory-store", "memory-store-contract"],
  ["task-storage", "TaskStorage", "swarmie-task-contracts", "task-contract"],
  ["durable-tasks", "DurableTasks", "swarmie-task-contracts", "task-contract"],
  ["task-handler", "TaskHandler", "swarmie-task-contracts", "task-contract"],
  [
    "durable-workflow",
    "DurableWorkflow",
    "swarmie-durable-tasks-fixture",
    "durable-fixture-contract",
  ],
  ["durable-gate", "TaskGate", "swarmie-durable-tasks-fixture", "durable-fixture-contract"],
] as const;
const watch = [
  "Cargo.toml",
  "Cargo.lock",
  ...[
    "crates/plugin-contracts",
    "crates/task-contracts",
    "crates/session-contracts",
    "tests/plugins/fixtures/counter",
    "tests/plugins/fixtures/durable-tasks",
    "plugins/memory-store",
  ].flatMap((owner) => [`${owner}/Cargo.toml`, ...new Bun.Glob(`${owner}/src/**/*.rs`).scanSync()]),
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
        ...(["memory-store", "durable-workflow"].includes(source) ? [] : ["--", source]),
      ],
      watch,
    },
    typescript: `target/generated/contracts/${source}.ts`,
    name,
  })),
  builds: {
    "session-client": {
      entry: "tests/plugins/fixtures/session-client/index.ts",
      platform: "server",
    },
    "durable-workflow-v1": {
      entry: "tests/plugins/fixtures/durable-tasks/v1.ts",
      platform: "server",
    },
    "durable-workflow-v2": {
      entry: "tests/plugins/fixtures/durable-tasks/v2.ts",
      platform: "server",
    },
    "counter-provider-gated": {
      entry: "tests/plugins/fixtures/provider-replacement/gated.ts",
      platform: "server",
    },
    "counter-provider-a": {
      entry: "tests/plugins/fixtures/provider-replacement/a.ts",
      platform: "server",
    },
    "counter-provider-b": {
      entry: "tests/plugins/fixtures/provider-replacement/b.ts",
      platform: "server",
    },
    "counter-provider-incompatible": {
      entry: "tests/plugins/fixtures/provider-replacement/incompatible.ts",
      platform: "server",
    },
    "counter-provider-failing": {
      entry: "tests/plugins/fixtures/provider-replacement/failing.ts",
      platform: "server",
    },
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
