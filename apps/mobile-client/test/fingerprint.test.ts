import { expect, test } from "bun:test";
import { fileURLToPath } from "node:url";

import { createFingerprintAsync } from "expo/fingerprint";

test("update signing certificates change the Mobile source fingerprint", async () => {
  const root = fileURLToPath(new URL("..", import.meta.url));
  const options = { platforms: ["ios" as const], ignorePaths: ["android/**/*", "ios/**/*"] };
  const original = await createFingerprintAsync(root, options);
  let changedCertificateSource = false;
  const changed = await createFingerprintAsync(root, {
    ...options,
    fileHookTransform(source, chunk, isEndOfFile) {
      if (source.type === "file" && source.filePath === "certs/development.pem" && isEndOfFile) {
        changedCertificateSource = true;
        return "\n// Changed certificate input for fingerprint qualification.\n";
      }
      return chunk;
    },
  });
  expect(changedCertificateSource).toBe(true);
  expect(changed.hash).not.toBe(original.hash);
  expect(
    original.sources.some(
      (source) => source.reasons.includes("updateSigningCertificates") && source.hash !== null,
    ),
  ).toBe(true);
}, 30_000);
