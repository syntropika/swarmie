import type { ClientExtension, UiSlot, ScopedClientApi } from "./contributions";
import { slotCatalog } from "./contributions";
import { Button } from "@/components/ui/button";
export type ContributionDemo = { extension: ClientExtension; transcription?: import("./dictation").TranscriptionProvider; mounts?: Map<string, (props: { api: ScopedClientApi }) => React.ReactNode> };
const compact: Partial<Record<UiSlot, string>> = {
  "sidebar.primary": "Guide", "sidebar.footer": "Demo active", "app.header.actions": "Guide",
  "conversation.header.actions": "Inspect chat", "composer.actions": "Insert example", "composer.context": "Demo context",
  "composer.status": "Demo • Ready to insert", "message.actions": "Inspect message", "message.details": "Demo message metadata",
  "project.actions": "Inspect", "project.contextMenu": "Inspect project", "conversation.contextMenu": "Inspect conversation",
  "panel.toolbar": "Refresh demo", "panel.status": "Demo panel • Active", "settings.sections": "Contribution demo",
};
function DemoContent({ api }: { api: ScopedClientApi }) {
  return <div className="dictation-settings"><p>This is a client contribution demo. The surrounding application is the real Swarmie interface; projects, messages, connection, and provider responses in this demo are fixtures.</p>
    <p>Use the same command from the composer, a context menu, the command palette, or Mod+Shift+I.</p>
    <Button variant="secondary" disabled={!api.context.composerReady} onClick={() => api.insertText("Example text from the contribution demo. ")}>Insert example at cursor</Button>
    <p>Conversation: {api.context.conversationId ?? "None"}</p><p>Message: {api.context.messageId ?? "None"}</p>
  </div>;
}
export const contributionDemo: ContributionDemo = {
  transcription: { id: "demo-fixture", available: () => true, start: (options) => {
    let timer: ReturnType<typeof setTimeout> | undefined;
    return { stop: () => { timer = setTimeout(() => { options.text("Fixture transcript. "); options.ended(); }, 900); }, abort: () => clearTimeout(timer) };
  } },
  extension: {
    id: "demo",
    manifest: { version: 1,
      commands: [
        { id: "inspect", title: "Inspect contribution context", when: [] },
        { id: "insert", title: "Insert example at cursor", when: ["composerReady"], shortcut: "Mod+Shift+i" },
        { id: "guide", title: "Open contribution demo screen", when: [] },
      ],
      slots: (Object.keys(slotCatalog) as UiSlot[]).map((slot) => {
        const kind = slot === "settings.sections" ? "section" : slotCatalog[slot].includes("text") ? "text" : "action";
        return { id: slot.replaceAll(".", "-"), slot, kind, title: compact[slot]!, text: kind !== "action" ? compact[slot] : undefined,
          command: kind === "action" ? slot === "composer.actions" ? "insert" : slot === "sidebar.primary" || slot === "app.header.actions" ? "guide" : "inspect" : undefined,
          order: 100, when: [], enabledWhen: [], clients: [] };
      }),
      screens: [{ id: "guide", title: "Contribution demo screen", clients: [] }], panels: [{ id: "inspector", title: "Contribution inspector", clients: [] }],
    },
    handlers: { inspect: (api) => api.openPanel("inspector"), insert: (api) => api.insertText("Example text from the contribution demo. "), guide: (api) => api.openScreen("guide") },
  },
  mounts: new Map([["demo/guide", DemoContent], ["demo/inspector", DemoContent], ["demo/settings-sections", DemoContent]]),
};
