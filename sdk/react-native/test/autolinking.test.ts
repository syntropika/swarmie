import { expect, test } from "bun:test";
import { spawnSync } from "node:child_process";
import path from "node:path";

test("Expo discovers the Swarmie native module for both platforms", () => {
  for (const platform of ["android", "apple"]) {
    const result = spawnSync(
      "node",
      [
        path.resolve("apps/mobile/node_modules/.bin/expo-modules-autolinking"),
        "resolve",
        "--platform",
        platform,
        "--project-root",
        "apps/mobile",
        "--json",
      ],
      { encoding: "utf-8" },
    );
    expect(result.status).toBe(0);
    const output: unknown = JSON.parse(result.stdout);
    if (
      typeof output !== "object" ||
      output === null ||
      !("modules" in output) ||
      !Array.isArray(output.modules)
    ) {
      throw new Error("Invalid Expo native-module discovery response");
    }
    expect(
      output.modules.some(
        (module: unknown) =>
          typeof module === "object" &&
          module !== null &&
          "packageName" in module &&
          module.packageName === "@swarmie/react-native",
      ),
    ).toBe(true);
  }
});
