import { build, loadConfig } from "@tembra/sdk/tooling";

import { packageFixtures } from "./package";

const mode = Bun.argv.at(2);
if (mode !== "contracts" && mode !== "plugins") {
  throw new Error("Usage: bun scripts/tembra/build.ts <contracts|plugins>");
}
const configuration = await loadConfig("examples/plugins/tembra.config.ts");
const result = await build(configuration, mode === "contracts" ? { targets: [] } : {});
if (mode === "plugins") {
  await packageFixtures(result.artifacts);
}
console.log(
  `Generated ${result.generated.files.length} contract bindings and built ${result.artifacts.length} artifacts`,
);
