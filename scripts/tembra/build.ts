import { build, loadConfig } from "@tembra/sdk/tooling";

import { packageFixtures } from "./package";

const mode = Bun.argv.at(2);
if (mode !== "contracts" && mode !== "plugins") {
  throw new Error("Usage: bun scripts/tembra/build.ts <contracts|plugins>");
}
// Hook descriptors come from the same Rust declarations as native execution. The SDK
// validates these descriptors when JavaScript contributions materialize them.
const hookExport = Bun.spawnSync([
  "cargo",
  "+1.99.0",
  "run",
  "--locked",
  "--quiet",
  "-p",
  "swarmie-agent-contracts",
  "--bin",
  "agent-contract",
  "--",
  "hooks",
]);
if (hookExport.exitCode !== 0) {
  throw new Error(new TextDecoder().decode(hookExport.stderr));
}
await Bun.write("target/generated/contracts/agent-hooks.json", hookExport.stdout);
const configuration = await loadConfig("tests/plugins/tembra.config.ts");
const result = await build(configuration, mode === "contracts" ? { targets: [] } : {});
if (mode === "plugins") {
  await packageFixtures(result.artifacts);
}
console.log(
  `Generated ${result.generated.files.length} contract bindings and built ${result.artifacts.length} artifacts`,
);
