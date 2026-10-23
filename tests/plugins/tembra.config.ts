import { defineConfig } from "@tembra/sdk/tooling";

const contracts = [
  ["model-provider", "ModelProviderSchema", "swarmie-model-contracts", "model-contract"],
  ["model-generation", "ModelGenerationSchema", "swarmie-model-contracts", "model-contract"],
  [
    "task-session-commit",
    "TaskSessionCommit",
    "swarmie-task-session-contracts",
    "task-session-contract",
  ],
  [
    "task-session-bindings",
    "TaskSessionBindings",
    "swarmie-task-session-contracts",
    "task-session-contract",
  ],
  [
    "task-session-publisher",
    "TaskSessionPublisher",
    "swarmie-task-session-contracts",
    "task-session-contract",
  ],
  ["agent-loop", "AgentLoop", "swarmie-agent-contracts", "agent-contract"],
  ["agent-observations", "AgentObservations", "swarmie-agent-contracts", "agent-contract"],
  ["agent-tool", "ToolHandler", "swarmie-agent-contracts", "agent-contract"],
  ["agent-tool-policy", "ToolPolicy", "swarmie-agent-contracts", "agent-contract"],
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
  ["task-created-observer", "TaskCreatedObserver", "swarmie-task-contracts", "task-contract"],
  [
    "task-phase-started-observer",
    "TaskPhaseStartedObserver",
    "swarmie-task-contracts",
    "task-contract",
  ],
  [
    "task-phase-settled-observer",
    "TaskPhaseSettledObserver",
    "swarmie-task-contracts",
    "task-contract",
  ],
  ["task-checkpoint-observer", "TaskCheckpointObserver", "swarmie-task-contracts", "task-contract"],
  ["task-waiting-observer", "TaskWaitingObserver", "swarmie-task-contracts", "task-contract"],
  ["task-sleeping-observer", "TaskSleepingObserver", "swarmie-task-contracts", "task-contract"],
  ["task-recovered-observer", "TaskRecoveredObserver", "swarmie-task-contracts", "task-contract"],
  ["task-blocked-observer", "TaskBlockedObserver", "swarmie-task-contracts", "task-contract"],
  ["task-parked-observer", "TaskParkedObserver", "swarmie-task-contracts", "task-contract"],
  ["task-retry-observer", "TaskRetryObserver", "swarmie-task-contracts", "task-contract"],
  [
    "task-cancellation-requested-observer",
    "TaskCancellationRequestedObserver",
    "swarmie-task-contracts",
    "task-contract",
  ],
  ["task-terminal-observer", "TaskTerminalObserver", "swarmie-task-contracts", "task-contract"],
  ["task-lifecycle", "TaskLifecycle", "swarmie-task-contracts", "task-contract"],
  [
    "durable-workflow",
    "DurableWorkflow",
    "swarmie-durable-tasks-fixture",
    "durable-fixture-contract",
  ],
  ["durable-gate", "TaskGate", "swarmie-durable-tasks-fixture", "durable-fixture-contract"],
] as const;
const selectors = new Map([
  ["model-provider", "provider"],
  ["model-generation", "generation"],
]);
const watch = [
  "Cargo.toml",
  "Cargo.lock",
  ...[
    "crates/plugin-contracts",
    "crates/task-contracts",
    "crates/session-contracts",
    "crates/model-contracts",
    "crates/agent-contracts",
    "crates/task-session-contracts",
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
        ...(["memory-store", "durable-workflow"].includes(source)
          ? []
          : ["--", selectors.get(source) ?? source]),
      ],
      watch,
    },
    typescript: `target/generated/contracts/${source}.ts`,
    name,
  })),
  builds: {
    "agent-context-v1": {
      entry: "tests/plugins/fixtures/agent-context/v1.ts",
      platform: "server",
    },
    "agent-context-v2": {
      entry: "tests/plugins/fixtures/agent-context/v2.ts",
      platform: "server",
    },
    "durable-observer-v1": {
      entry: "tests/plugins/fixtures/durable-tasks/observer-v1.ts",
      platform: "server",
    },
    "durable-observer-v2": {
      entry: "tests/plugins/fixtures/durable-tasks/observer-v2.ts",
      platform: "server",
    },
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
