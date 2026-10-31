import { useSource } from "../../../packages/client-ui/src/index";
import { NodeProtocol } from "../../../plugins/client-node/browser/src/index";
import { browserToolDetailsPersistence } from "../../../plugins/settings/browser/src/index";
import { Button } from "@swarmie/ui-web";
import { setThemePreference } from "@swarmie/ui-web/theme";
import { createHost } from "@syntropika/tembra-sdk/browser";
import { useState } from "react";
import { ProjectsContext } from "../../../plugins/projects/browser/src/context";
import { createRoot } from "react-dom/client";

import { ClientContext } from "../../../apps/web-client/src/client/context";
import { initializeClient } from "../../../apps/web-client/src/client/initialize";
import { ConversationPanel } from "../../../apps/web-client/src/conversations/conversation";
import { ConversationSettings } from "../../../apps/web-client/src/settings/conversation";

import "../../../apps/web-client/src/styles.css";

const client = await initializeClient(await createHost(), {
  dev: false,
  toolDetailsPersistence: browserToolDetailsPersistence(),
  nodes: [{ id: "fixture", label: "Fixture node", principal: "fixture-user", url: null }],
  connector: async (node) => ({
    identity: { nodeId: node.id, principal: node.principal, protocolVersion: NodeProtocol.Version },
    get: async () => { throw new Error("No remote service is used in this presentation fixture."); },
    close: () => undefined,
  }),
});
const chat = client.activeConversation();
chat.messages.update(() => [
  { id: "user", role: "user", text: "Inspect the project settings." },
  { id: "first", role: "assistant", text: "I will check the available configuration." },
  { id: "result-a", role: "tool", text: '{"section":"Appearance","available":true}' },
  { id: "result-b", role: "tool", text: '{"section":"Conversation","available":true}', truncated: true },
  { id: "last", role: "assistant", text: "The display preferences are available in Conversation settings." },
]);
chat.execution.update((current) => ({ ...current, phase: "working", label: "Thinking…" }));
setThemePreference(new URLSearchParams(location.search).get("theme") === "light" ? "light" : "dark");

function Fixture() {
  const [view, setView] = useState("conversation");
  const execution = useSource(chat.execution);
  const settings = useSource(client.settings);
  return (
    <ClientContext value={client}>
      <ProjectsContext value={client.projectsPresentation}>
      <main className="flex h-full min-h-0 flex-col bg-pane text-foreground">
        <nav aria-label="Fixture controls" className="flex shrink-0 flex-wrap items-center gap-2 p-3">
          <Button variant="secondary" size="sm" onPress={() => setView(view === "conversation" ? "settings" : "conversation")}>
            {view === "conversation" ? "Show settings" : "Show conversation"}
          </Button>
          <Button variant="ghost" size="sm" onPress={() => chat.execution.update((current) => ({ ...current, phase: "working", label: "Thinking…", canAcknowledge: false }))}>Working</Button>
          <Button variant="ghost" size="sm" onPress={() => chat.execution.update((current) => ({ ...current, phase: "completed", label: "", canAcknowledge: false }))}>Complete</Button>
          <Button variant="ghost" size="sm" onPress={() => chat.execution.update((current) => ({ ...current, phase: "unknown", label: "The request admission is not confirmed.", canAcknowledge: false }))}>Unknown admission</Button>
          <Button variant="ghost" size="sm" onPress={() => chat.execution.update((current) => ({ ...current, phase: "unknown", label: "A tool outcome is unknown.", canAcknowledge: true }))}>Unknown effect</Button>
          <output aria-label="Fixture state" className="text-caption">{execution.phase} / {settings.toolDetails}</output>
        </nav>
        <div className="min-h-0 flex-1">
          {view === "conversation" ? <ConversationPanel chat={chat} /> : (
            <div className="h-full overflow-auto p-6"><ConversationSettings /></div>
          )}
        </div>
      </main>
    </ProjectsContext>
    </ClientContext>
  );
}

const root = document.querySelector("#root");
if (root === null) throw new Error("The fixture root is missing.");
createRoot(root).render(<Fixture />);
