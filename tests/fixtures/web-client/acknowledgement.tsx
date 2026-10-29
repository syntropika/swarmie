import { createHost } from "@syntropika/tembra-sdk/browser";
import { createRoot } from "react-dom/client";

import { ClientContext } from "../../../apps/web-client/src/client/context";
import { initializeClient } from "../../../apps/web-client/src/client/initialize";
import type { ClientUi } from "../../../apps/web-client/src/client/initialize";
import { ConversationStatus } from "../../../apps/web-client/src/conversations/status";
import { MessageRow } from "../../../apps/web-client/src/conversations/message-row";
import { useSource } from "../../../packages/client-ui/src/index";
import { NodeProtocol } from "../../../plugins/connections/browser/src/index";

import "../../../apps/web-client/src/styles.css";

const counters = { submissions: 0, acknowledgements: 0 };
const acknowledgement = Promise.withResolvers<undefined>();
const client: ClientUi = await initializeClient(await createHost(), {
  dev: false,
  nodes: [{ id: "fixture", label: "Fixture", principal: "fixture-user", url: null }],
  connector: async (node) => ({
    identity: { nodeId: node.id, principal: node.principal, protocolVersion: NodeProtocol.Version },
    get: async () => { throw new Error("This fixture uses the public execution adapter."); },
    close: () => undefined,
  }),
  execution: () => ({
    submit: async () => { counters.submissions += 1; return { accepted: true }; },
    stop: async () => undefined,
    async acknowledge(id) {
      counters.acknowledgements += 1;
      const chat = client.roster.getSnapshot().find((entry) => entry.id === id);
      if (chat === undefined) {
        throw new Error("The fixture conversation is missing.");
      }
      chat.execution.update((value) => ({ ...value, label: "Acknowledgement pending" }));
      await acknowledgement.promise;
      chat.execution.update((value) => ({ ...value, phase: "failed", label: "Unknown outcome acknowledged. No tool was replayed.", canAcknowledge: false }));
    },
  }),
});
const chat = client.activeConversation();
client.providers.state.update((state) => ({ ...state, accounts: [{
  id: "account", providerId: "fixture", providerLabel: "Fixture", label: "Fixture",
  available: true, models: [{ id: "model", label: "Model", efforts: [] }],
}] }));
chat.draft.update((draft) => ({ ...draft, text: "First request", account: "account", model: "model" }));
async function ready(remaining = 100): Promise<void> {
  if (client.canSend(chat)) {
    return;
  }
  if (remaining === 0) {
    throw new Error("The fixture draft did not become ready.");
  }
  const frame = Promise.withResolvers<undefined>();
  requestAnimationFrame(() => { frame.resolve(undefined); });
  await frame.promise;
  await ready(remaining - 1);
}
await ready();
client.send(chat);
async function admitted(remaining = 100): Promise<void> {
  if (chat.started) {
    return;
  }
  if (remaining === 0) {
    throw new Error("The fixture submission was not admitted.");
  }
  const frame = Promise.withResolvers<undefined>();
  requestAnimationFrame(() => { frame.resolve(undefined); });
  await frame.promise;
  await admitted(remaining - 1);
}
await admitted();
chat.draft.update((draft) => ({ ...draft, text: "Keep this next draft" }));
chat.execution.update((value) => ({ ...value, phase: "unknown", canAcknowledge: true, label: "Unknown tool outcome" }));

function Probe() {
  const execution = useSource(chat.execution);
  const draft = useSource(chat.draft);
  return <ClientContext value={client}>
    <main className="mx-auto grid w-full max-w-xl min-w-0 gap-5 p-6">
      <h1>Acknowledgement interaction fixture</h1>
      <div className="flex flex-wrap gap-3">
        <button type="button" onClick={() => { chat.execution.update((value) => ({ ...value, phase: "unknown", canAcknowledge: false })); }}>Show unknown admission</button>
        <button type="button" onClick={() => { chat.execution.update((value) => ({ ...value, phase: "unknown", canAcknowledge: true })); }}>Show recoverable outcome</button>
        <button type="button" onClick={() => { chat.execution.update((value) => ({ ...value, phase: "failed", canAcknowledge: true })); }}>Show failed phase</button>
        <button type="button" onClick={() => { acknowledgement.resolve(undefined); }}>Settle acknowledgement</button>
      </div>
      <ConversationStatus chat={chat} />
      <MessageRow chat={chat} message={{ id: "shortened", role: "assistant", text: "Bounded content", truncated: true }} />
      <output aria-label="Fixture counters" className="min-w-0 break-all">{JSON.stringify({ ...counters, phase: execution.phase, canAcknowledge: execution.canAcknowledge, draft: draft.text })}</output>
    </main>
  </ClientContext>;
}
const root = document.querySelector("#root");
if (root === null) {
  throw new Error("The fixture root is missing.");
}
document.documentElement.classList.add("dark");
createRoot(root).render(<Probe />);
