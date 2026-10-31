import { useSource } from "@swarmie/client-ui";
import type { SecretMetadata, VaultCard } from "@swarmie/plugin-vault-browser";
import { Button, Input, Label, TextField } from "@swarmie/ui-web";
import { useState } from "react";
import type { SubmitEvent } from "react";

import { useClient } from "../client/context";
import { VaultPager } from "./paging";

const scope = (value: SecretMetadata["projects"], all: string) =>
  value.kind === "unrestricted" ? all : `Only: ${value.ids.join(", ") || "none"}`;

const approvalLabels = {
  none: "No additional approval",
  operation: "Every operation",
  conversation: "This operation or the main agent in this conversation",
};
function Policy({ metadata, replacing }: { metadata: SecretMetadata; replacing: boolean }) {
  return (
    <dl className="mt-3 grid gap-2 text-xs">
      {replacing ? (
        <div className="font-medium">Replaces the existing value of {metadata.name}.</div>
      ) : null}
      <div>
        <dt className="text-muted">Projects</dt>
        <dd className="break-all">{scope(metadata.projects, "All projects")}</dd>
      </div>
      <div>
        <dt className="text-muted">Agents</dt>
        <dd className="break-all">{scope(metadata.agents, "All agents")}</dd>
      </div>
      <div>
        <dt className="text-muted">Future use approval</dt>
        <dd>{approvalLabels[metadata.approval]}</dd>
      </div>
      {metadata.target_account !== null && metadata.target_account !== "" ? (
        <div>
          <dt className="text-muted">Target account</dt>
          <dd>{metadata.target_account}</dd>
        </div>
      ) : null}
    </dl>
  );
}
function PrivateCard({ card }: { card: VaultCard }) {
  const { vault } = useClient();
  const [busy, setBusy] = useState(false);
  async function submit(event: SubmitEvent<HTMLFormElement>) {
    event.preventDefault();
    const form = event.currentTarget;
    const input = form.elements.namedItem("secret");
    if (!(input instanceof HTMLInputElement) || !input.value || busy) {
      return;
    }
    setBusy(true);
    const pending = vault.submitEntry(card.id, input.value);
    input.value = "";
    try {
      await pending;
    } finally {
      setBusy(false);
    }
  }
  async function decide(choice: "allow-once" | "conversation" | "cancel") {
    setBusy(true);
    try {
      await vault.decide(card.id, choice);
    } finally {
      setBusy(false);
    }
  }
  return (
    <section
      aria-label={card.title}
      className="rounded-xl border border-border bg-chrome-surface p-4"
    >
      <h2 className="text-sm font-medium">{card.title}</h2>
      <p className="mt-1 text-xs leading-5 text-muted">{card.purpose}</p>
      {card.kind === "entry" || card.kind === "capture" || card.kind === "use" ? (
        <Policy metadata={card.metadata} replacing={card.kind !== "use" && card.replacing} />
      ) : null}
      {card.kind === "use" ? (
        <div className="mt-3 space-y-1 text-xs break-all">
          <p>
            {card.mainAgent ? "Main agent" : "Child agent"} · {card.agent ?? "Ad-hoc agent"}
          </p>
          <p>Project: {card.project ?? "No project"}</p>
          <p>{card.reference}</p>
          <p>Provider: {card.provider}</p>
        </div>
      ) : null}
      {card.kind === "entry" ? (
        <form
          onSubmit={(event) => {
            void submit(event).catch(() => false);
          }}
          className="mt-3 space-y-3"
        >
          <TextField name="secret" type="password" isRequired isDisabled={busy} autoComplete="off">
            <Label>{card.replacing ? "Replacement value" : "Secret value"}</Label>
            <Input maxLength={65_536} />
          </TextField>
          <p className="text-xs text-muted">
            Sent privately to your node and encrypted. The agent receives a reference.
          </p>
          <div className="flex gap-2">
            <Button type="submit" isDisabled={busy}>
              Save privately
            </Button>
            <Button
              variant="tertiary"
              isDisabled={busy}
              onPress={() => {
                void decide("cancel").catch(() => false);
              }}
            >
              Cancel
            </Button>
          </div>
        </form>
      ) : (
        <>
          <p className="mt-3 text-xs break-all">{card.destination}</p>
          {card.kind === "model" ? (
            <p role="alert" className="mt-3 text-xs leading-5 text-danger">
              {card.inspection === "detected"
                ? `Possible credentials detected: ${card.rules.join(", ")}.`
                : "This content could not be fully inspected."}{" "}
              Sending it can disclose private information to this model provider.
            </p>
          ) : null}
          {card.kind === "capture" ? (
            <p className="mt-3 text-xs text-muted">
              Approve creating and storing one secret. The value and remaining producer output stay
              private. Issuance may have effects even if saving fails.
            </p>
          ) : null}
          <div className="mt-4 flex flex-wrap gap-2">
            <Button
              isDisabled={busy}
              onPress={() => {
                void decide("allow-once").catch(() => false);
              }}
            >
              {
                {
                  model: "Send anyway, once",
                  capture: "Create and save",
                  use: "Allow this operation",
                  entry: "Save privately",
                }[card.kind]
              }
            </Button>
            {card.kind === "use" && card.conversationAllowed ? (
              <Button
                variant="secondary"
                isDisabled={busy}
                onPress={() => {
                  void decide("conversation").catch(() => false);
                }}
              >
                Allow main agent in this conversation
              </Button>
            ) : null}
            <Button
              variant="tertiary"
              isDisabled={busy}
              onPress={() => {
                void decide("cancel").catch(() => false);
              }}
            >
              Cancel
            </Button>
          </div>
        </>
      )}
    </section>
  );
}
export function VaultCards({ conversation }: { conversation: string | undefined }) {
  const { vault } = useClient();
  const state = useSource(vault.state);
  const cards = state.cards.filter((card) => card.conversation === conversation);
  const pending = conversation === undefined ? 0 : (state.cardCounts[conversation] ?? 0);
  if (cards.length === 0 && pending === 0) {
    return null;
  }
  return (
    <div className="mx-auto w-full max-w-[720px] space-y-3 px-4 pb-3" aria-live="polite">
      {cards.length < pending ? (
        <p className="text-xs text-muted">More private requests are available on another page.</p>
      ) : null}
      {cards.map((card) => (
        <PrivateCard key={card.id} card={card} />
      ))}
      <VaultPager section="cards" />
    </div>
  );
}
export function VaultGlobalCards() {
  const { vault } = useClient();
  const state = useSource(vault.state);
  return (
    <div className="mt-6 space-y-3">
      {state.cards
        .filter((card) => card.conversation === null)
        .map((card) => (
          <PrivateCard key={card.id} card={card} />
        ))}
      <VaultPager section="cards" />
    </div>
  );
}
