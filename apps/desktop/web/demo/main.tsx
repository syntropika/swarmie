import { createRoot } from "react-dom/client";

import { NodeWorkspace } from "../src/app";
import type { NodesState } from "../src/nodes";
import { newThread } from "../src/protocol";
import type { Thread } from "../src/protocol";
import { WorkspaceStore } from "../src/workspace";
import type { Workspace } from "../src/workspace";
import { contributionDemo } from "./contributions";
import { DemoClient } from "./demo-client";

import "../src/styles.css";
import "./styles.css";

const node = { environmentId: "demo", storeId: "demo-store", ownerScopeId: "demo-owner" };
const first: Thread = {
  ...newThread("demo-project", "demo-worktree", "codex"),
  id: "demo-chat",
  title: "Contribution contract",
  sessionId: "demo-session",
  draft: "Draft text with a cursor insertion point.",
  messages: [
    {
      id: "demo-question",
      role: "user",
      content: [{ type: "text", text: "How can dictation work across all harnesses?" }],
      createdAt: new Date("2026-01-01T10:00:00Z"),
    },
    {
      id: "demo-answer",
      role: "assistant",
      content: [
        {
          type: "text",
          text: "Capture audio on the **client displaying the interface**. Register a composer action and dispatch a shared command.\n\nA replaceable transcription provider returns text through a scoped composer API. The selected harness receives the same ordinary draft.",
        },
      ],
    },
  ],
  context: { usedTokens: 12_400, contextWindowTokens: 200_000 },
};
const state: Workspace = {
  node,
  disabledHarnesses: [],
  projects: [
    {
      id: "demo-project",
      name: "Extension playground",
      path: "demo-project",
      git: true,
      worktrees: [
        {
          id: "demo-worktree",
          path: "demo-worktree",
          branch: "main",
          isMain: true,
          locked: false,
          prunable: false,
        },
      ],
    },
  ],
  threads: [
    first,
    {
      ...newThread("demo-project", "demo-worktree", "claude"),
      id: "demo-second",
      submitted: true,
      sessionId: "demo-other-session",
      title: "Another conversation",
    },
  ],
  activeId: first.id,
  harnesses: ["codex", "claude", "opencode", "gemini"].map((harness) => ({
    harness,
    available: true,
    capabilities: {},
  })),
  connected: true,
  git: true,
  busy: false,
  error: "",
  extensions: [],
  cacheSaving: false,
  notifications: {
    enabled: false,
    completion: false,
    input: false,
    failure: false,
    backgroundOnly: true,
  },
  chatDirectory: "demo-directory",
};
class DemoStore extends WorkspaceStore {
  private readonly demoListeners = new Set<() => void>();
  private demoState: Workspace;
  // Client calls keep asynchronous acceptance even though the fixture owner is ready immediately.
  private readonly ownerReady = Promise.resolve();
  constructor(demoState: Workspace) {
    super();
    this.demoState = demoState;
  }
  override getSnapshot = () => this.demoState;
  override subscribe = (listener: () => void) => {
    this.demoListeners.add(listener);
    return () => {
      this.demoListeners.delete(listener);
    };
  };
  private patch(next: Partial<Workspace>) {
    this.demoState = { ...this.demoState, ...next };
    for (const listener of this.demoListeners) {
      listener();
    }
  }
  override updateThread = (id: string, changes: Partial<Thread>) => {
    this.patch({
      threads: this.demoState.threads.map((thread) =>
        thread.id === id ? { ...thread, ...changes } : thread,
      ),
    });
  };
  override select(id: string) {
    this.patch({ activeId: id });
  }
  override active() {
    return this.demoState.threads.find((thread) => thread.id === this.demoState.activeId);
  }
  override setError(error: unknown) {
    this.patch({ error: String(error) });
  }
  override dismissError() {
    this.patch({ error: "" });
  }
  private async readiness() {
    await this.ownerReady;
    if (!this.demoState.connected) {
      throw new Error("The demo node is disconnected");
    }
  }
  override async previewControls() {
    await this.readiness();
  }
  override async loadMessageTimes() {
    await this.readiness();
  }
  override async refreshExtensions() {
    await this.readiness();
  }
  override createThread() {
    const thread = {
      ...newThread("demo-project", "demo-worktree", "codex"),
      id: crypto.randomUUID(),
    };
    this.patch({ threads: [...this.demoState.threads, thread], activeId: thread.id });
  }
  override async rename(id: string, title: string) {
    this.updateThread(id, { title });
    await this.readiness();
  }
  override async archive(id: string, archived: boolean) {
    this.updateThread(id, { archived });
    await this.readiness();
  }
  override async send(id: string, text: string) {
    const thread = this.demoState.threads.find((candidate) => candidate.id === id);
    if (!thread) {
      throw new Error("Unknown demo conversation");
    }
    this.updateThread(id, {
      draft: "",
      messages: [
        ...thread.messages,
        { id: crypto.randomUUID(), role: "user", content: [{ type: "text", text }] },
      ],
    });
    await this.readiness();
  }
  override async invokeExtension<T>(): Promise<T> {
    await this.readiness();
    throw new Error(
      `Runtime commands are not connected for ${this.demoState.node?.environmentId ?? "this demo"}.`,
    );
  }
  override async review() {
    this.patch({ review: { diff: "", files: [], truncated: false, filesTruncated: false } });
    await this.readiness();
  }
}
const store = new DemoStore(state);
const nodes: NodesState = {
  store,
  nodes: [
    {
      identity: node,
      displayName: "Demo client",
      local: true,
      registered: true,
      lastVerifiedAtMs: 0,
      connections: [],
    },
  ],
  loading: false,
  error: "",
};
const client = new DemoClient(nodes);
const root = document.querySelector("#root");
if (!root) {
  throw new Error("Contribution demo root is missing");
}
createRoot(root).render(
  <>
    <div className="contribution-demo-notice">
      Contribution demo • UI is implemented; workspace data and transcription are fixtures.
    </div>
    <NodeWorkspace
      store={store}
      nodes={nodes}
      client={client}
      clientContributions={[contributionDemo]}
      transcriptionProvider={contributionDemo.transcription}
    />
  </>,
);
