import type { Snapshot } from "@swarmie/client-ui";
import { updatePresentation } from "@swarmie/client-ui";
import { orb } from "@syntropika/tembra-sdk";

export type SecretMetadata = {
  name: string;
  purpose: string;
  projects: { kind: "unrestricted" } | { kind: "only"; ids: string[] };
  agents: { kind: "unrestricted" } | { kind: "only"; ids: string[] };
  approval: "none" | "operation" | "conversation";
  policy_generation: string;
  target_account: string | null;
};
export type SecretSummary = {
  reference: string;
  revision: string;
  metadata: SecretMetadata;
};
export type VaultCard = {
  id: string;
  conversation: string | null;
  title: string;
  purpose: string;
} & (
  | { kind: "entry"; metadata: SecretMetadata; replacing: boolean }
  | {
      kind: "use";
      destination: string;
      conversationAllowed: boolean;
      metadata: SecretMetadata;
      reference: string;
      provider: string;
      project: string | null;
      agent: string | null;
      mainAgent: boolean;
    }
  | {
      kind: "capture";
      destination: string;
      profile: string;
      metadata: SecretMetadata;
      replacing: boolean;
    }
  | {
      kind: "model";
      destination: string;
      inspection: "detected" | "incomplete" | "unavailable";
      rules: string[];
    }
);
export type VaultIdentity = { account: string; vault: string; epoch: string };
export type EnrollmentRequest = {
  identity: VaultIdentity;
  device: string;
  signing: number[];
  encryption: number[];
};
export type EnrollmentStatement = {
  identity: VaultIdentity;
  recipient: string;
  signing_key: number[];
  approver: number[];
  roles: string[];
};
export type SignedEnrollment = { statement: EnrollmentStatement; signature: number[] };
export type JoinProposal = {
  certificate: SignedEnrollment;
  chain: SignedEnrollment[];
  root: number[];
};
export type LifecycleRequest =
  | { kind: "resolve"; operation: string }
  | { kind: "begin" | "rebind" | "select" | "inspect"; identity: VaultIdentity }
  | { kind: "prepare"; request: EnrollmentRequest; verified_request_code: string }
  | { kind: "accept"; proposal: JoinProposal; verified_proposal_code: string }
  | { kind: "approve"; offer: string; verified_proposal_code: string }
  | { kind: "cancel_offer"; offer: string }
  | { kind: "cancel_join"; identity: VaultIdentity; device: string }
  | {
      kind: "remove";
      identity: VaultIdentity;
      target: string;
      compromised: boolean;
      affected: string[];
      authority_base: number;
    }
  | { kind: "preview_removal"; identity: VaultIdentity; target: string; compromised: boolean }
  | { kind: "restore"; identity: VaultIdentity; expected_current: string };
export type MutationSummary = {
  latest: string | null;
  reference: { account: string; vault: string; secret: string };
  expected: string | null;
  metadata: SecretMetadata;
  kind: "value" | "deleted";
  prepared_entry: string;
};
export type CaptureSummary = {
  latest: string | null;
  id: string;
  state: string;
  reference: { account: string; vault: string; secret: string };
  metadata: SecretMetadata;
  expected: string | null;
  has_candidate: boolean;
  has_submission: boolean;
};
export type CaptureAction =
  | { kind: "recognize" | "fence" | "store" | "discard" }
  | { kind: "rebase"; metadata: SecretMetadata; expected: string | null };
export type RecoveryRequest =
  | { kind: "recognize_mutation" | "fence_mutation" | "discard_draft"; mutation: string }
  | { kind: "retry_draft"; mutation: string; metadata: SecretMetadata; expected: string | null }
  | { kind: "capture"; capture: string; action: CaptureAction };
export type VaultPages = Record<
  string,
  { offset: number; total: number; next: number | null; previous: number | null }
>;
export type LifecycleState = {
  pages: VaultPages;
  removal: {
    identity: VaultIdentity;
    target: string;
    compromised: boolean;
    affected: string[];
    authority_base: number;
  } | null;
  roots: {
    identity: VaultIdentity;
    exposed: boolean;
    enrolled: boolean;
    verifiedCode: string | null;
    pendingReplacement: { device: string; verifiedCode: string } | null;
    certificates: SignedEnrollment[];
    removed: string[];
  }[];
  joins: { request: EnrollmentRequest; code: string; keysUnavailable?: boolean }[];
  offers: { id: string; proposal: JoinProposal; code: string }[];
  pending: { identity: VaultIdentity; operation: string }[];
};
/** Only safe projections are serializable. Credentials never enter this State. */
export type VaultState = {
  pages: VaultPages;
  phase: "unavailable" | "signed-out" | "locked" | "unlocking" | "ready" | "joining";
  entries: SecretSummary[];
  cards: VaultCard[];
  cardCounts: Record<string, number>;
  synchronization: "offline" | "synchronizing" | "confirmed";
  archived: boolean;
  exposed: boolean;
  detectorEnabled: boolean;
  detectorAvailable: boolean;
  captures: CaptureSummary[];
  mutations: MutationSummary[];
  rejected: [MutationSummary, string][];
  lifecycle: LifecycleState;
  pendingFoundings: { vault: string; operation: string }[];
  authority: {
    identity: VaultIdentity;
    sequence: number;
    devices: { device: string; removed: boolean; roles: string[] }[];
  } | null;
  needsSignIn: boolean;
  signingIn: boolean;
  cloud: { api: string; authentication: string } | null;
  current: string | null;
  canFound: boolean;
  error: string | null;
};
export const unavailableVault: VaultState = {
  phase: "unavailable",
  pages: {},
  needsSignIn: false,
  signingIn: false,
  mutations: [],
  rejected: [],
  pendingFoundings: [],
  authority: null,
  lifecycle: { roots: [], joins: [], offers: [], pending: [], pages: {}, removal: null },
  entries: [],
  cards: [],
  cardCounts: {},
  synchronization: "offline",
  archived: false,
  exposed: false,
  detectorEnabled: true,
  detectorAvailable: false,
  captures: [],
  cloud: null,
  current: null,
  canFound: false,
  error: null,
};

/** Complete private shell port. This object belongs only to the authenticated native UI channel.
 * It is never registered as an agent tool, persisted, or included in inspector State.
 */
export type VaultBackend = {
  state: Snapshot<Readonly<VaultState>>;
  signIn: (endpoints: { api: string; authentication: string }) => Promise<void>;
  page: (key: string, offset: number) => Promise<void>;
  lifecycle: (request: LifecycleRequest) => Promise<void>;
  recovery: (request: RecoveryRequest) => Promise<void>;
  resolveFounding: (vault: string) => Promise<void>;
  found: (expectedCurrent: string | null) => Promise<void>;
  signOut: () => Promise<void>;
  unlock: () => Promise<void>;
  synchronize: () => Promise<void>;
  save: (
    metadata: SecretMetadata,
    value: string,
    existing?: { reference: string; revision: string },
  ) => Promise<void>;
  remove: (entry: SecretSummary) => Promise<void>;
  submitEntry: (card: string, value: string) => Promise<void>;
  decide: (card: string, choice: "allow-once" | "conversation" | "cancel") => Promise<void>;
  configureDetector: (enabled: boolean) => Promise<void>;
  close: () => void;
};
export const vaultUiOrb = orb({
  name: "swarmie.vault.ui",
  apiVersion: 1,
  services: {
    "swarmie.vault.ui": {
      create: (self) => self.state<VaultState>(unavailableVault),
      methods: { update: updatePresentation<VaultState> },
    },
  },
});

export function createVaultUi(
  state: Snapshot<Readonly<VaultState>>,
  update: (value: VaultState) => void,
  initialBackend?: VaultBackend,
) {
  let closed = false;
  let owner = initialBackend;
  const fail = () => {
    if (!closed) {
      update({
        ...state.getSnapshot(),
        error: "The private operation could not finish. Check its status before trying again.",
      });
    }
  };
  async function run(action: (owner: VaultBackend) => Promise<void>) {
    if (closed || !owner) {
      return false;
    }
    try {
      await action(owner);
      return true;
    } catch {
      fail();
      return false;
    }
  }
  const copy = () => {
    if (!closed && owner) {
      update({ ...owner.state.getSnapshot() });
    }
  };
  let detach = owner?.state.subscribe(copy);
  copy();
  return {
    state,
    get available() {
      return owner !== undefined;
    },
    attach(backend: VaultBackend) {
      if (closed) {
        backend.close();
        return;
      }
      detach?.();
      owner?.close();
      owner = backend;
      detach = owner.state.subscribe(copy);
      copy();
    },
    signIn: async (endpoints: { api: string; authentication: string }) =>
      await run(async (target) => {
        await target.signIn(endpoints);
      }),
    page: async (key: string, offset: number) =>
      await run(async (target) => {
        await target.page(key, offset);
      }),
    lifecycle: async (request: LifecycleRequest) =>
      await run(async (target) => {
        await target.lifecycle(request);
      }),
    recovery: async (request: RecoveryRequest) =>
      await run(async (target) => {
        await target.recovery(request);
      }),
    resolveFounding: async (vault: string) =>
      await run(async (target) => {
        await target.resolveFounding(vault);
      }),
    found: async (expected: string | null) =>
      await run(async (target) => {
        await target.found(expected);
      }),
    signOut: async () =>
      await run(async (target) => {
        await target.signOut();
      }),
    unlock: async () =>
      await run(async (target) => {
        await target.unlock();
      }),
    synchronize: async () =>
      await run(async (target) => {
        await target.synchronize();
      }),
    save: async (
      metadata: SecretMetadata,
      value: string,
      existing?: { reference: string; revision: string },
    ) =>
      await run(async (target) => {
        await target.save(metadata, value, existing);
      }),
    remove: async (entry: SecretSummary) =>
      await run(async (target) => {
        await target.remove(entry);
      }),
    submitEntry: async (card: string, value: string) =>
      await run(async (target) => {
        await target.submitEntry(card, value);
      }),
    decide: async (card: string, choice: "allow-once" | "conversation" | "cancel") =>
      await run(async (target) => {
        await target.decide(card, choice);
      }),
    configureDetector: async (enabled: boolean) =>
      await run(async (target) => {
        await target.configureDetector(enabled);
      }),
    close() {
      closed = true;
      detach?.();
      owner?.close();
    },
  };
}

export { readVaultState, readEnrollmentRequest, readJoinProposal } from "./validation";
