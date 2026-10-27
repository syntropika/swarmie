import type { Snapshot } from "@swarmie/client-ui";
import { updatePresentation } from "@swarmie/client-ui";
import { orb } from "@syntropika/tembra-sdk";

export type ProviderModel = { id: string; label: string; description?: string; efforts: string[] };
export type ProviderAccount = {
  id: string;
  providerId: string;
  providerLabel: string;
  label: string;
  models: ProviderModel[];
  available: boolean;
};
export type ProviderConfigurationMethod = {
  id: string;
  label: string;
  kind: "api-key" | "local" | "oauth";
};
export type ProviderChoice = {
  id: string;
  label: string;
  publisher: string;
  methods: ProviderConfigurationMethod[];
};
export type ProvidersState = {
  choices: ProviderChoice[];
  accounts: ProviderAccount[];
  error: string | null;
  ready: boolean;
};
/** Reusable provider configuration, independent of any onboarding flow. */
export type ProviderConfigurationApi = {
  configure: (
    input: { providerId: string; method: string; credential?: string; url?: string },
    signal: AbortSignal,
  ) => Promise<ProviderAccount>;
};
export type ProviderDiscoveryApi = {
  detect: (signal: AbortSignal) => Promise<string[]>;
};
export type ProviderAuthorizationApi = {
  begin: (
    input: { providerId: string; method: string },
    signal: AbortSignal,
  ) => Promise<ProviderAuthorizationSession>;
};
export const providersOrb = orb({
  name: "swarmie.providers.ui",
  apiVersion: 1,
  services: {
    "swarmie.providers.ui": {
      create: (self) =>
        self.state<ProvidersState>({ choices: [], accounts: [], error: null, ready: false }),
      methods: { update: updatePresentation<ProvidersState> },
    },
  },
});

/** The node owns authorization and credential storage. The browser observes a public projection. */
export type ProviderAuthorizationState =
  | {
      kind: "waiting";
      authorizationUrl: string;
      userCode: string | null;
      callbackUri: string | null;
    }
  | { kind: "connected"; account: ProviderAccount }
  | { kind: "failed"; error: string };
export type ProviderAuthorizationSession = Snapshot<ProviderAuthorizationState> & {
  complete: (callbackUrl: string) => Promise<void>;
  cancel: () => Promise<void>;
  dispose: () => void;
};

export { createProvidersApi } from "./api";
export type { ProvidersApi, ProvidersBackend } from "./api";
export type { ProviderSelection } from "./selection";
