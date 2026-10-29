import catalog from "../../catalog.json";
import type { ProviderChoice, ProviderConfigurationMethod } from "./index";

/** Hosts publish their available choices; presentation fixtures use this same product catalog. */
export function providerCatalog(): ProviderChoice[] {
  return catalog.map((provider) => ({
    ...provider,
    methods: provider.methods.map((method): ProviderConfigurationMethod => {
      if (method.kind !== "api-key" && method.kind !== "local" && method.kind !== "oauth") {
        throw new Error("The bundled provider catalog has an invalid connection kind.");
      }
      return { ...method, kind: method.kind };
    }),
  }));
}
