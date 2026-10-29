import { expect, test } from "bun:test";

import { providerAccount } from "../src/client/provider-values";

function account(label: string) {
  return {
    id: "account",
    providerId: "openai",
    providerLabel: "OpenAI",
    label,
    available: true,
    models: [{ id: "model", label: "Model", efforts: ["medium"] }],
  };
}

test("provider labels use the native UTF-8 byte limit", () => {
  expect(providerAccount(account("é".repeat(256))).label).toBe("é".repeat(256));
  expect(() => providerAccount(account("é".repeat(257)))).toThrow("invalid provider account");
  expect(() => providerAccount(account("   "))).toThrow("invalid provider account");
});

test("the browser accepts bounded model previews and rejects invalid model identifiers", () => {
  expect(providerAccount({ ...account("Account"), modelsTruncated: true })).toMatchObject({
    modelsTruncated: true,
  });
  expect(() => providerAccount({ ...account("Account"), modelsTruncated: "yes" })).toThrow(
    "invalid provider account",
  );
  expect(() =>
    providerAccount({
      ...account("Account"),
      models: [{ id: "é".repeat(128), label: "Model", efforts: [] }],
    }),
  ).toThrow("invalid model");
});
