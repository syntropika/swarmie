import { useSource } from "@swarmie/client-ui";
import type { ComposerTopBarItem, Conversation } from "@swarmie/plugin-conversations-browser";
import { Check, Paperclip, Plus, Puzzle, ShieldCheck } from "lucide-react";
import { useRef, useState } from "react";

import { useClient } from "../../client/context";
import { Caption, IconButton, Item, Menu } from "../../controls";
import { useComposerActions } from "./actions";
import { ComposerAccount, ComposerNode } from "./context";
import { ComposerModel } from "./model";
import { ComposerSubmit } from "./submit";

const baseTopBar: readonly ComposerTopBarItem[] = [
  {
    id: "swarmie.providers.account",
    owner: "providers",
    side: "left",
    priority: 100,
    component: ComposerAccount,
  },
  {
    id: "swarmie.connections.node",
    owner: "connections",
    side: "right",
    priority: 100,
    component: ComposerNode,
  },
];

export function Composer({ chat }: { chat: Conversation }) {
  const { canSend, connectionState, conversationNode, conversationsUi, providers, navigate, send } =
    useClient();

  const draft = useSource(chat.draft);
  const execution = useSource(chat.execution);
  const connections = useSource(connectionState);
  const status = connections.sessions[conversationNode(chat)]?.phase ?? "disconnected";
  const providerState = useSource(providers.state);
  const items = useSource(conversationsUi.composer.topBar.items);
  const topBar = [...baseTopBar, ...items];
  const { actions, commands, attachments, actionContext, executeAction } = useComposerActions(chat);
  const fileInput = useRef<HTMLInputElement>(null);
  const [mode, setMode] = useState<"queue" | "steer">("queue");
  const account = providerState.accounts.find((entry) => entry.id === draft.account);
  const models = account?.models ?? [];
  const busy = ["queued", "working", "stopping"].includes(execution.phase);
  const slash = draft.text.startsWith("/") && !draft.text.includes(" ");
  const patch = (change: Partial<typeof draft>) => {
    chat.draft.update((current) => ({ ...current, ...change }));
  };
  return (
    <div className="@container/composer shrink-0 px-4 pt-2 pb-4 md:px-6 md:pb-5">
      <div className="relative mx-auto w-full max-w-[720px]">
        {slash ? (
          <section
            aria-label="Slash commands"
            className="absolute inset-x-0 bottom-full z-30 mb-2 max-h-72 overflow-y-auto rounded-xl border border-border bg-overlay p-1.5 shadow-lg"
          >
            <Caption>Commands</Caption>
            {commands
              .filter((command) => command.command.includes(draft.text.slice(1)))
              .map((command) => (
                <Item
                  key={command.id}
                  icon={command.icon}
                  detail={`/${command.command}`}
                  onClick={() => {
                    patch({ text: "" });
                    executeAction(command);
                  }}
                >
                  {command.label}
                </Item>
              ))}
            {commands.length === 0 ? (
              <p className="px-2 py-3 text-xs text-muted">No commands are available.</p>
            ) : null}
          </section>
        ) : null}
        <div className="overflow-hidden rounded-xl border border-composer-border bg-composer">
          <div className="flex h-8 items-center justify-between gap-2 border-b border-border bg-composer-context px-2 text-chrome text-muted">
            {(["left", "right"] as const).map((side) => (
              <div
                key={side}
                className={`flex min-w-0 items-center gap-0.5 ${side === "right" ? "ml-auto shrink-0 flex-row-reverse" : "overflow-x-auto"}`}
              >
                {topBar
                  .filter((item) => item.side === side)
                  .toSorted((a, b) => b.priority - a.priority || a.id.localeCompare(b.id))
                  .map(({ id, component: Control }) => (
                    <Control key={id} conversationId={chat.id} isDraft={!chat.started} />
                  ))}
              </div>
            ))}
          </div>
          {draft.attachments.length ? (
            <div className="flex flex-wrap gap-1.5 px-3 pt-2">
              {draft.attachments.map((name) => (
                <span
                  key={name}
                  className="flex items-center gap-1 rounded-md bg-default px-2 py-1 text-chrome"
                >
                  <Paperclip className="size-3" />
                  {name}
                  <IconButton
                    icon={Plus}
                    size="tabClose"
                    label={`Remove ${name}`}
                    onPress={() => {
                      patch({ attachments: draft.attachments.filter((entry) => entry !== name) });
                    }}
                  />
                </span>
              ))}
            </div>
          ) : null}
          <label className="sr-only" htmlFor={`composer-${chat.id}`}>
            Message
          </label>
          <textarea
            id={`composer-${chat.id}`}
            rows={2}
            value={draft.text}
            onChange={(event) => {
              patch({ text: event.target.value });
            }}
            onKeyDown={(event) => {
              if (
                event.key === "Enter" &&
                !event.shiftKey &&
                !event.nativeEvent.isComposing &&
                !slash &&
                canSend(chat)
              ) {
                event.preventDefault();
                send(chat, mode);
              }
            }}
            placeholder="Ask anything…"
            className="block min-h-20 w-full resize-none border-0 bg-transparent px-3 pt-3 pb-2 text-base leading-6 text-foreground placeholder:text-muted @min-[640px]/composer:text-conversation"
          />
          <div className="flex items-center gap-1 px-2 pb-2">
            <Menu label="Add to conversation" icon={Plus} size="composer" placement="top start">
              {(close) => (
                <>
                  <Caption>Add</Caption>
                  <Item
                    icon={Paperclip}
                    onClick={() => {
                      fileInput.current?.click();
                      close();
                    }}
                  >
                    Files and folders
                  </Item>
                  {[...new Set(attachments.map((action) => action.section))].map((section) => (
                    <div key={section}>
                      <Caption>{section}</Caption>
                      {attachments
                        .filter((action) => action.section === section)
                        .map((action) => (
                          <Item
                            key={action.id}
                            icon={action.icon}
                            onClick={() => {
                              executeAction(action);
                              close();
                            }}
                          >
                            {action.label}
                          </Item>
                        ))}
                    </div>
                  ))}
                  <Caption>Plugins</Caption>
                  <Item
                    icon={Puzzle}
                    onClick={() => {
                      navigate({ section: "settings", settings: "plugins" });
                      close();
                    }}
                  >
                    Manage plugins
                  </Item>
                </>
              )}
            </Menu>
            <input
              ref={fileInput}
              type="file"
              multiple
              aria-label="Attach files"
              className="hidden"
              onChange={(event) => {
                patch({
                  attachments: [
                    ...new Set([
                      ...draft.attachments,
                      ...[...(event.target.files ?? [])].map((file) => file.name),
                    ]),
                  ],
                });
              }}
            />
            <Menu
              label="Permissions"
              icon={ShieldCheck}
              text={draft.permission}
              size="composerCap"
              placement="top start"
            >
              {(close) => (
                <>
                  {["Ask", "Full access"].map((permission) => (
                    <Item
                      key={permission}
                      icon={permission === draft.permission ? Check : ShieldCheck}
                      onClick={() => {
                        patch({ permission });
                        close();
                      }}
                    >
                      {permission}
                    </Item>
                  ))}
                </>
              )}
            </Menu>
            {actions
              .filter((action) => action.slot === "leading")
              .map((action) => (
                <IconButton
                  key={action.id}
                  icon={action.icon}
                  size="composer"
                  label={action.label}
                  disabled={action.enabled?.(actionContext()) === false}
                  onPress={() => {
                    executeAction(action);
                  }}
                />
              ))}
            <div className="flex-1" />
            {busy ? (
              <Menu
                label="Message delivery"
                text={mode === "queue" ? "Queue" : "Steer"}
                size="composerCap"
                placement="top end"
              >
                {(close) => (
                  <>
                    {(["queue", "steer"] as const).map((entry) => (
                      <Item
                        key={entry}
                        onClick={() => {
                          setMode(entry);
                          close();
                        }}
                      >
                        {entry === "queue" ? "Queue for the next turn" : "Steer at the next step"}
                      </Item>
                    ))}
                  </>
                )}
              </Menu>
            ) : null}
            <ComposerModel
              draft={draft}
              models={models}
              onSelectModel={(id) => {
                const selection = providers.selectModel(draft, id);
                if (selection) {
                  patch(selection);
                }
              }}
              onSelectEffort={(effort) => {
                patch({ effort });
              }}
            />
            {actions
              .filter((action) => action.slot === "before-send")
              .map((action) => (
                <IconButton
                  key={action.id}
                  icon={action.icon}
                  size="composer"
                  label={action.label}
                  disabled={action.enabled?.(actionContext()) === false}
                  onPress={() => {
                    executeAction(action);
                  }}
                />
              ))}
            <ComposerSubmit chat={chat} mode={mode} />
          </div>
        </div>
        {status === "connecting" || status === "reconnecting" ? (
          <output className="mt-2 flex items-center justify-center gap-2 text-xs text-muted">
            <span className="size-2 animate-pulse rounded-full border border-brand-accent" />
            Connecting to your node…
          </output>
        ) : null}
        {execution.label ? (
          <output className="mt-2 text-center text-xs text-muted">{execution.label}</output>
        ) : null}
      </div>
    </div>
  );
}
