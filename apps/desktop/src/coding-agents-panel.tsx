import { useQueries } from "@tanstack/react-query";
import { Check, Copy, ExternalLink, Info, Monitor, RefreshCw } from "lucide-react";
import { useState, useSyncExternalStore } from "react";

import { startAction, errorMessage } from "~/src/action-boundary";
import { AgentToolConnection } from "~/src/agent-tool-connection";
import { sortedCopy } from "~/src/collection-order";
import { Button } from "~/src/components/ui/button";
import { Switch } from "~/src/components/ui/switch";
import { HarnessIcon } from "~/src/harness-select";
import { harnessGuide, harnessReadiness, primaryAgentTools } from "~/src/harness-setup";
import { nodeKey } from "~/src/node-scope";
import { copyText } from "~/src/protocol";
import { hasText } from "~/src/value-presence";
import type { WorkspaceStore } from "~/src/workspace";

export function CodingAgentsPanel(props: { store: WorkspaceStore; nodeName: string }) {
  const state = useSyncExternalStore(props.store.subscribe, props.store.getSnapshot);
  const scope = state.node ? nodeKey(state.node) : "";
  return <AgentToolsPanel key={`${scope}:${state.connected}`} {...props} />;
}
function AgentToolsPanel({ store, nodeName }: { store: WorkspaceStore; nodeName: string }) {
  const state = useSyncExternalStore(store.subscribe, store.getSnapshot);
  const [expanded, setExpanded] = useState<string>();
  const [showAll, setShowAll] = useState(false);
  const [errors, setErrors] = useState<Record<string, string>>({});
  const [copied, setCopied] = useState("");
  const scope = state.node ? nodeKey(state.node) : "";
  const probes = useQueries({
    queries: state.harnesses.map((item) => ({
      queryKey: ["agent-tools-readiness", scope, item.harness],
      queryFn: async () => {
        await store.probe(item.harness);
        return null;
      },
      enabled: state.connected && item.available,
    })),
  });
  const checking = state.harnesses
    .filter((_, index) => probes.at(index)?.isFetching === true)
    .map((item) => item.harness);
  const check = async (harnesses: string[]) => {
    await Promise.all(
      harnesses.map(async (harness) => {
        const index = state.harnesses.findIndex((item) => item.harness === harness);
        const probe = probes.at(index);
        if (probe !== undefined) {
          await probe.refetch();
        }
      }),
    );
  };
  const primary = primaryAgentTools;
  const rank = (id: string) => (primary.includes(id) ? primary.indexOf(id) : primary.length);
  const visible = sortedCopy(
    state.harnesses.filter((item) => showAll || primary.includes(item.harness)),
    (a, b) => rank(a.harness) - rank(b.harness),
  );
  const hidden = state.harnesses.length - visible.length;
  const enabledCount = state.harnesses.filter(
    (item) => item.available && !state.disabledHarnesses.includes(item.harness),
  ).length;

  return (
    <div className="coding-agents-panel">
      <div className="onboarding-node">
        <Monitor size={15} aria-hidden="true" />
        <span>{nodeName}</span>
        <span className={`connection-dot ${state.connected ? "connected" : ""}`} />
        <span>{state.connected ? "Connected" : "Disconnected"}</span>
      </div>
      {!state.connected && (
        <output className="onboarding-note">
          Reconnect to check the agent tools on this computer.
        </output>
      )}
      <fieldset aria-label="Agent tools" className="onboarding-harnesses">
        {visible.map((item) => {
          const guide = harnessGuide(item.harness);
          const status = harnessReadiness(item);
          const pending = checking.includes(item.harness);
          const probe = probes.at(
            state.harnesses.findIndex((candidate) => candidate.harness === item.harness),
          );
          const issue =
            errors[item.harness] ?? (probe?.isError === true ? errorMessage(probe.error) : "");
          const enabled = item.available && !state.disabledHarnesses.includes(item.harness);
          return (
            <div className="onboarding-harness" key={item.harness}>
              <div className="onboarding-harness-row">
                <span className="onboarding-harness-icon">
                  <HarnessIcon harness={item.harness} />
                </span>
                <div className="onboarding-harness-copy">
                  <span className="onboarding-harness-name">{guide.name}</span>
                  <p>
                    {(() => {
                      if (pending) {
                        return "Checking installation and login…";
                      }
                      if (issue) {
                        return "Check unavailable · Try again";
                      }
                      return status.detail;
                    })()}
                  </p>
                </div>
                <output
                  className={`onboarding-harness-status ${enabled && status.ready && !pending && !issue ? "ready" : ""}`}
                >
                  {(() => {
                    if (pending) {
                      return (
                        <>
                          <RefreshCw
                            size={13}
                            className="animate-spin motion-reduce:animate-none"
                          />
                          Checking
                        </>
                      );
                    }
                    if (issue) {
                      return "Unknown";
                    }
                    if (item.available) {
                      if (!enabled) {
                        return "Disabled";
                      }
                      if (status.ready) {
                        return (
                          <>
                            <Check size={14} />
                            Ready
                          </>
                        );
                      }
                      return status.label;
                    }
                    return status.label;
                  })()}
                </output>
                {!pending && (
                  <Button
                    variant="outline"
                    size="sm"
                    aria-expanded={expanded === item.harness}
                    onClick={() => {
                      setExpanded(expanded === item.harness ? undefined : item.harness);
                      setCopied("");
                    }}
                  >
                    {(() => {
                      if (item.available) {
                        if (status.ready) {
                          return "Manage connection";
                        }
                        return "Connect";
                      }
                      return "Install";
                    })()}
                  </Button>
                )}
                <Switch
                  label={`Use ${guide.name}`}
                  checked={enabled}
                  hideLabel
                  disabled={!state.node || !item.available}
                  onChange={(value) => {
                    try {
                      store.setHarnessEnabled(item.harness, value);
                      setErrors((previous) => ({ ...previous, [item.harness]: "" }));
                    } catch (error) {
                      setErrors((previous) => ({
                        ...previous,
                        [item.harness]: errorMessage(error),
                      }));
                    }
                  }}
                />
              </div>
              {expanded === item.harness &&
                (() => {
                  const guideContent = (
                    <>
                      {item.available && <p>Use a terminal on {nodeName}.</p>}
                      <p>
                        {item.available
                          ? guide.instruction
                          : `Install ${guide.name} on ${nodeName}, then check again.`}
                      </p>
                      {item.available && hasText(guide.command) && (
                        <div className="onboarding-command">
                          <code>{guide.command}</code>
                          <Button
                            variant="ghost"
                            size="sm"
                            aria-label={`Copy ${guide.name} command`}
                            onClick={() => {
                              const { command } = guide;
                              if (!hasText(command)) {
                                return;
                              }
                              const copy = async () => {
                                await copyText(command);
                                setCopied(item.harness);
                              };
                              startAction(copy(), () => {
                                setErrors((previous) => ({
                                  ...previous,
                                  [item.harness]:
                                    "Could not copy the command. Select and copy it manually.",
                                }));
                              });
                            }}
                          >
                            {copied === item.harness ? <Check size={14} /> : <Copy size={14} />}
                            {copied === item.harness ? "Copied" : "Copy"}
                          </Button>
                        </div>
                      )}
                      <div className="onboarding-guide-actions">
                        {hasText(guide.documentation) && (
                          <a href={guide.documentation} target="_blank" rel="noreferrer">
                            {item.available ? "Setup guide" : "Installation guide"}
                            <ExternalLink size={12} />
                          </a>
                        )}
                        <Button
                          variant="secondary"
                          size="sm"
                          disabled={!state.connected || pending}
                          onClick={() => {
                            startAction(check([item.harness]), store.setError.bind(store));
                          }}
                        >
                          Check again
                        </Button>
                      </div>
                    </>
                  );
                  return (
                    <AgentToolConnection
                      key={`${scope}:${item.harness}`}
                      store={store}
                      harness={item.harness}
                      name={guide.name}
                      nodeName={nodeName}
                      onClose={() => {
                        setExpanded(undefined);
                      }}
                      fallback={guideContent}
                    />
                  );
                })()}
              {issue && (
                <p role="alert" className="onboarding-row-error">
                  {issue}
                </p>
              )}
            </div>
          );
        })}
        {!visible.length && (
          <p className="onboarding-empty">
            No agent tools have been reported by this computer yet.
          </p>
        )}
      </fieldset>
      <div className="onboarding-list-actions">
        {(() => {
          if (hidden > 0) {
            return (
              <Button
                variant="ghost"
                size="sm"
                onClick={() => {
                  setShowAll(true);
                }}
              >
                Show {hidden} more agent tools
              </Button>
            );
          }
          if (showAll) {
            return (
              <Button
                variant="ghost"
                size="sm"
                onClick={() => {
                  setShowAll(false);
                }}
              >
                Show primary agent tools
              </Button>
            );
          }
          return <span />;
        })()}
        <Button
          variant="ghost"
          size="sm"
          disabled={!state.connected || checking.length > 0}
          onClick={() => {
            startAction(
              check(store.getSnapshot().harnesses.map((item) => item.harness)),
              store.setError.bind(store),
            );
          }}
        >
          <RefreshCw size={13} />
          Check again
        </Button>
      </div>
      <p className="onboarding-note">
        <Info size={15} aria-hidden="true" />
        <span>
          {enabledCount === 0
            ? "Enable an agent tool to start a new chat. You can do this later in Settings."
            : "Enabled agent tools are available for new chats. You can change this anytime in Settings."}
        </span>
      </p>
    </div>
  );
}
