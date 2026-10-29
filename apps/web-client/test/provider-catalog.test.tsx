import { expect, test } from "bun:test";

import { providerCatalog } from "@swarmie/plugin-providers-browser";
import { ProviderLogo } from "@swarmie/ui-web";
import { renderToStaticMarkup } from "react-dom/server";

test("the shared provider catalog uses the approved names and stable node identities", () => {
  const choices = providerCatalog();
  expect(choices.map(({ id, label }) => [id, label])).toEqual([
    ["openai", "ChatGPT"],
    ["anthropic", "Claude"],
    ["kimi", "Kimi"],
    ["ollama", "Local"],
  ]);
  const cloned = providerCatalog();
  choices.pop();
  expect(cloned).toHaveLength(4);
  expect(cloned[0]?.methods.map((method) => method.id)).toEqual(["oauth-browser", "api-key"]);
});

test("native provider ids and presentation ids use the same provider marks", () => {
  expect(renderToStaticMarkup(<ProviderLogo providerId="anthropic" />)).toBe(
    renderToStaticMarkup(<ProviderLogo providerId="claude" />),
  );
  expect(renderToStaticMarkup(<ProviderLogo providerId="ollama" />)).toBe(
    renderToStaticMarkup(<ProviderLogo providerId="local" />),
  );
});
