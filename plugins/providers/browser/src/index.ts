import type { Snapshot } from "@swarmie/client-ui";
import { updatePresentation } from "@swarmie/client-ui";
import { orb } from "@syntropika/tembra-sdk";

export type ProviderModel = { id: string; label: string; description?: string; efforts: string[] };
export type ModelDefaults = {
  model: string | null;
  effort: string | null;
  auxiliaryModel: string | null;
};
export const automaticModelDefaults: ModelDefaults = {
  model: null,
  effort: null,
  auxiliaryModel: null,
};
export type ProviderAccount = {
  id: string;
  providerId: string;
  providerLabel: string;
  label: string;
  models: ProviderModel[];
  available: boolean;
  defaults?: ModelDefaults;
  /** The mutation response contains a bounded preview. Load the catalog for all models. */
  modelsTruncated?: boolean;
  authorizationMethod?: string;
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
  scope: { nodeId: string; principal: string; generation: number } | null;
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
export type ModelDefaultsApi = {
  save: (accountId: string, defaults: ModelDefaults, signal: AbortSignal) => Promise<ModelDefaults>;
};
export type ProviderAuthorizationApi = {
  begin: (
    input: { providerId: string; method: string; accountId?: string },
    signal: AbortSignal,
  ) => Promise<ProviderAuthorizationSession>;
};
export const providersOrb = orb({
  name: "swarmie.providers.ui",
  apiVersion: 1,
  services: {
    "swarmie.providers.ui": {
      create: (self) =>
        self.state<ProvidersState>({
          scope: null,
          choices: [],
          accounts: [],
          error: null,
          ready: false,
        }),
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
  | { kind: "failed"; error: string; retryable?: boolean };
export type ProviderAuthorizationSession = Snapshot<ProviderAuthorizationState> & {
  complete: (callbackUrl: string) => Promise<void>;
  cancel: () => Promise<void>;
  dispose: () => void;
  retry?: () => Promise<void>;
};

export { createProvidersApi } from "./api";
export { providerCatalog } from "./catalog";
export type { ProvidersApi, ProvidersBackend } from "./api";
export type { ProviderSelection } from "./selection";
export { providerDisplayName } from "./presentation";
