import { expect, test } from "bun:test";
import { fileURLToPath } from "node:url";

import { createFingerprintAsync } from "expo/fingerprint";

test("the OTA runtime changes when the Rust client changes", async () => {
  const root = fileURLToPath(new URL("..", import.meta.url));
  const options = { platforms: ["ios" as const], ignorePaths: ["android/**/*", "ios/**/*"] };
  const original = await createFingerprintAsync(root, options);
  let changedRustSource = false;
  const changed = await createFingerprintAsync(root, {
    ...options,
    fileHookTransform(source, chunk, isEndOfFile) {
      if (
        source.type === "file" &&
        source.filePath === "../../crates/client/src/lib.rs" &&
        isEndOfFile
      ) {
        changedRustSource = true;
        return "\n// Changed native client input for compatibility qualification.\n";
      }
      return chunk;
    },
  });
  expect(changedRustSource).toBe(true);
  expect(changed.hash).not.toBe(original.hash);
  for (const reason of [
    "swarmieNativeClient",
    "swarmieNativeBoundary",
    "updateSigningCertificates",
  ]) {
    expect(
      original.sources.some((source) => source.reasons.includes(reason) && source.hash !== null),
    ).toBe(true);
  }
}, 30_000);
