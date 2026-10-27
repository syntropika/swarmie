import { createHost } from "@syntropika/tembra-sdk/browser";
import { createRoot } from "react-dom/client";
import { ClientContext } from "../../../apps/client/src/client/context";
import { App } from "../../../apps/client/src/shell/layout";
import { initializeClient } from "../../../apps/client/src/client/initialize";
import "../../../apps/client/src/styles.css";

// Explicit authoring fixture: no credentials, model request, server or durable task is simulated as production.
const host = await createHost();
const client = await initializeClient(host, { dev: true });
client.providers.state.update(() => ({ ready: true, error: null, choices: [
  { id: "openai", label: "ChatGPT", publisher: "OpenAI", methods: [{ id: "API key", label: "API key", kind: "api-key" }] },
  { id: "claude", label: "Claude", publisher: "Anthropic", methods: [{ id: "API key", label: "API key", kind: "api-key" }] },
  { id: "kimi", label: "Kimi", publisher: "Moonshot", methods: [{ id: "API key", label: "API key", kind: "api-key" }] },
  { id: "local", label: "Local", publisher: "Ollama", methods: [{ id: "Ollama", label: "Ollama", kind: "local" }, { id: "OpenAI compatible", label: "OpenAI compatible", kind: "local" }] },
], accounts: [{ id: "fixture-account", providerId: "openai", providerLabel: "ChatGPT", label: "Example account", available: true, models: [
  { id: "fixture-balanced", label: "Balanced model", description: "Everyday work and conversation", efforts: ["Low", "Medium", "High"] },
  { id: "fixture-fast", label: "Fast model", description: "Quick responses for simple tasks", efforts: [] },
] }] }));
const chat = client.activeConversation();
client.renameConversation(chat, "Shape the project");
const project = await client.projects.create({ name: "Swarmie" });
await client.projects.assignConversation({ projectId: project.id, conversationId: chat.id });
client.conversationState.update((current) => ({ ...current, records: current.records.map((record) => record.id === chat.id ? { ...record, started: true, draft: { ...record.draft, account: "fixture-account", model: "fixture-balanced" }, messages: [
  { id: "fixture-user", role: "user", text: "Keep the conversation simple and give every extension a clear home." },
  { id: "fixture-assistant", role: "assistant", text: "The conversation stays at the center. Open a view when you need one.\n\nEach plugin contributes through the component it extends, while the node owns execution and history." },
] } : record) }));
Object.assign(window, { fixtureClient: client });
const root = document.querySelector("#root");
if (!root) throw new Error("Missing fixture root.");
createRoot(root).render(<ClientContext value={client}><App /></ClientContext>);
