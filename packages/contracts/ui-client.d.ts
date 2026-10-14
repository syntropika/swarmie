import type { UiClient, UiCommand, UiContribution, UiSurface } from "./index.js";

export type ContributionContext = Readonly<{
  client: UiClient;
  nodeId?: string;
  conversationId?: string;
  projectId?: string;
  worktreeId?: string;
  sessionId?: string;
  messageId?: string;
  panelId?: string;
  connected: boolean;
  composerReady: boolean;
}>;
export type ContributionEvent =
  | { type: "context.changed"; context: ContributionContext }
  | { type: "composer.changed"; context: ContributionContext }
  | {
      type: "surface.opened" | "surface.closed";
      kind: "screen" | "panel";
      id: string;
      context: ContributionContext;
    };
export type ScopedClientApi = {
  readonly signal: AbortSignal;
  readonly context: ContributionContext;
  dispose(): void;
  insertText(text: string, options?: { separateWords?: boolean }): void;
  openScreen(id: string): void;
  openPanel(id: string): void;
  on(type: ContributionEvent["type"], callback: (event: ContributionEvent) => void): () => void;
};
export type CommandHandler = (api: ScopedClientApi) => unknown | Promise<unknown>;
// Authoring forms allow the same omitted defaults that runtime manifests deserialize.
export type ClientCommand = Omit<UiCommand, "when"> & Partial<Pick<UiCommand, "when">>;
export type ClientContribution = Omit<
  UiContribution,
  "clients" | "when" | "enabledWhen" | "order"
> &
  Partial<Pick<UiContribution, "clients" | "when" | "enabledWhen" | "order">>;
export type ClientSurface = Omit<UiSurface, "clients"> & Partial<Pick<UiSurface, "clients">>;
export type ClientManifest = {
  version: number;
  commands?: ClientCommand[];
  slots?: ClientContribution[];
  screens?: ClientSurface[];
  panels?: ClientSurface[];
};
export type ClientExtension = {
  id: string;
  manifest: ClientManifest;
  handlers?: Record<string, CommandHandler>;
  activate?: (lifecycle: { signal: AbortSignal; on: ScopedClientApi["on"] }) => void | (() => void);
};
