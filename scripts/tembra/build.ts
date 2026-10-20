import process from "node:process";

/** Root-owned adapter to Tembra's programmatic tooling; no runtime loading occurs here. */
import { build, loadConfig } from "@tembra/sdk/tooling";

const configuration = process.argv.at(2);
if (configuration === undefined || process.argv.length !== 3) {
  throw new Error("Usage: bun scripts/tembra/build.ts <configuration>");
}
const result = await build(await loadConfig(configuration));
console.log(
  JSON.stringify(
    Object.fromEntries(result.artifacts.map((output) => [output.target, output.artifactPath])),
  ),
);
