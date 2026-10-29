import { providerCatalog } from "./catalog";

const providers = providerCatalog();

/** Product names come from the shared catalog without changing execution identities. */
export function providerDisplayName(id: string, label: string): string {
  return providers.find((provider) => provider.id === id)?.label ?? label;
}
