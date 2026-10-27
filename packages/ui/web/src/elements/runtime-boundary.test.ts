import { expect, test } from "bun:test";

// Resolve from the repository root, as required by the shared JavaScript workflow.
test("the built public component graph does not import an assistant runtime", () => {
  const result = Bun.spawnSync([process.execPath, "scripts/ui-elements/verify.mjs"], {
    cwd: process.cwd(),
    stdout: "pipe",
    stderr: "pipe",
  });
  expect(new TextDecoder().decode(result.stderr)).toBe("");
  expect(result.exitCode).toBe(0);
  expect(JSON.parse(new TextDecoder().decode(result.stdout))).toMatchObject({
    success: true,
    assistantRuntimeModules: 0,
  });
});
