import { createRoot } from "react-dom/client";
import { NodeWorkspace } from "../src/App";
import { contributionDemo } from "./contributions";
import { newThread, type Thread } from "../src/protocol";
import type { WorkspaceStore, Workspace } from "../src/workspace";
import type { NodesState, NodeClient } from "../src/nodes";
import "../src/styles.css";
import "./styles.css";

const node = { environmentId: "demo", storeId: "demo-store", ownerScopeId: "demo-owner" };
const first: Thread = { ...newThread("demo-project", "demo-worktree", "codex"), id: "demo-chat", title: "Contribution contract", sessionId: "demo-session", draft: "Draft text with a cursor insertion point.", messages: [
  { id: "demo-question", role: "user", content: [{ type: "text", text: "How can dictation work across all harnesses?" }], createdAt: new Date("2026-01-01T10:00:00Z") },
  { id: "demo-answer", role: "assistant", content: [{ type: "text", text: "Capture audio on the **client displaying the interface**. Register a composer action and dispatch a shared command.\n\nA replaceable transcription provider returns text through a scoped composer API. The selected harness receives the same ordinary draft." }] },
], context: { usedTokens: 12400, contextWindowTokens: 200000 } };
let state: Workspace = {
  node, projects: [{ id: "demo-project", name: "Extension playground", path: "demo-project", git: true, worktrees: [{ id: "demo-worktree", path: "demo-worktree", branch: "main", isMain: true, locked: false, prunable: false }] }],
  threads: [first, { ...newThread("demo-project", "demo-worktree", "claude"), id: "demo-second", submitted: true, sessionId: "demo-other-session", title: "Another conversation" }], activeId: first.id,
  harnesses: ["codex", "claude", "opencode", "gemini"].map((harness) => ({ harness, available: true, capabilities: {} })),
  connected: true, git: true, busy: false, error: "", extensions: [], cacheSaving: false,
  notifications: { enabled: false, completion: false, input: false, failure: false, backgroundOnly: true }, chatDirectory: "demo-directory",
};
const listeners = new Set<() => void>();
const patch = (next: Partial<Workspace>) => { state = { ...state, ...next }; listeners.forEach((listener) => listener()); };
const update = (id: string, changes: Partial<Thread>) => patch({ threads: state.threads.map((thread) => thread.id === id ? { ...thread, ...changes } : thread) });
const store = {
  getSnapshot: () => state, subscribe: (listener: () => void) => { listeners.add(listener); return () => listeners.delete(listener); },
  updateThread: update, select: (id: string) => patch({ activeId: id }), active: () => state.threads.find((thread) => thread.id === state.activeId),
  setError: (error: unknown) => patch({ error: String(error) }), dismissError: () => patch({ error: "" }),
  previewControls: async () => {}, loadMessageTimes: async () => {}, refreshExtensions: async () => {},
  createThread: () => { const thread = { ...newThread("demo-project", "demo-worktree", "codex"), id: crypto.randomUUID() }; patch({ threads: [...state.threads, thread], activeId: thread.id }); },
  rename: async (id: string, title: string) => update(id, { title }), archive: async (id: string, archived: boolean) => update(id, { archived }),
  send: async (id: string, text: string) => update(id, { draft: "", messages: [...state.threads.find((thread) => thread.id === id)!.messages, { id: crypto.randomUUID(), role: "user", content: [{ type: "text", text }] }] }),
  invokeExtension: async () => { throw new Error("Runtime commands are not connected in this fixture."); },
  review: async () => patch({ review: { diff: "", files: [], truncated: false, filesTruncated: false } }),
} as unknown as WorkspaceStore;
const nodes: NodesState = { store, nodes: [{ identity: node, displayName: "Demo client", local: true, registered: true, lastVerifiedAtMs: 0, connections: [] }], loading: false, error: "" };
const client = { select: async () => {}, getSnapshot: () => nodes } as unknown as NodeClient;
createRoot(document.getElementById("root")!).render(<><div className="contribution-demo-notice">Contribution demo • UI is implemented; workspace data and transcription are fixtures.</div><NodeWorkspace store={store} nodes={nodes} client={client} clientContributions={[contributionDemo]} transcriptionProvider={contributionDemo.transcription} /></>);
