import { useEffect, useState, useCallback, useId } from "react";

import { useExtensionQuery } from "~/web/src/components/extension-query";
import { extensionResult } from "~/web/src/components/extension-result";
import { hasCount, hasText } from "~/web/src/components/presentation-values";
import { Button } from "~/web/src/components/ui/button";
import { Checkbox } from "~/web/src/components/ui/checkbox";
import { Disclosure } from "~/web/src/components/ui/disclosure";
import { Input } from "~/web/src/components/ui/input";
import { Select } from "~/web/src/components/ui/select";
import { Textarea } from "~/web/src/components/ui/textarea";
import { HarnessSelect } from "~/web/src/harness-select";
import type { SessionSnapshot, Thread } from "~/web/src/protocol";
import type { WorkspaceStore } from "~/web/src/workspace";

type Schedule = {
  id: string;
  sessionId: string;
  text: string;
  nextRunAtMs: number;
  intervalSeconds?: number;
  enabled: boolean;
  status: string;
  error?: string;
};
export function Schedules({ store, thread }: { store: WorkspaceStore; thread: Thread }) {
  const fieldPrefix = useId();

  const [text, setText] = useState("");
  const [at, setAt] = useState("");
  const [intervalValue, setIntervalValue] = useState("");
  const [editing, setEditing] = useState<Schedule>();
  const [busy, setBusy] = useState(false);
  const run = useCallback(
    async (action: () => Promise<unknown>) => {
      setBusy(true);
      try {
        await action();
      } catch (error) {
        store.setError(error);
      } finally {
        setBusy(false);
      }
    },
    [store],
  );
  const fetchSchedules = useCallback(async () => {
    const result = await store.invokeExtension<{ schedules: Schedule[] }>(
      "resources",
      "schedules.list",
      {},
      extensionResult<{ schedules: Schedule[] }>({
        type: "object",
        properties: {
          schedules: {
            type: "array",
            items: {
              type: "object",
              properties: {
                id: { type: "string" },
                sessionId: { type: "string" },
                text: { type: "string" },
                nextRunAtMs: { type: "number" },
                intervalSeconds: { type: "number" },
                enabled: { type: "boolean" },
                status: { type: "string" },
                error: { type: "string" },
              },
              required: ["id", "sessionId", "text", "nextRunAtMs", "enabled", "status"],
            },
          },
        },
        required: ["schedules"],
      }),
    );
    return result.schedules;
  }, [store]);
  const catalog = useExtensionQuery(store, ["schedules"], fetchSchedules);
  const schedules = catalog.data ?? [];
  const load = catalog.refresh;
  return (
    <div className="panel-body">
      <h3>Scheduled prompts</h3>
      <p className="panel-note">
        Runs while the daemon is active. Busy conversations receive a queued prompt; permissions
        stay interactive.
      </p>
      <Button
        size="sm"
        variant="ghost"

        disabled={busy}
        onClick={() => {
          run(load).catch(store.setError.bind(store));
        }}
      >
        Refresh schedules
      </Button>
      {schedules.map((schedule) => (
        <div className="schedule-row" key={schedule.id}>
          <strong>{schedule.text.slice(0, 100)}</strong>
          <p className="muted">
            {schedule.status} · {new Date(schedule.nextRunAtMs).toLocaleString()}
            {hasCount(schedule.intervalSeconds)
              ? ` · every ${schedule.intervalSeconds / 60} minutes`
              : ""}
          </p>
          {hasText(schedule.error) && <p className="inline-error">{schedule.error}</p>}
          <div className="permission-actions">
            <Button
              size="sm"
              variant="ghost"

              disabled={busy}
              onClick={() => {
                run(async () => {
                  await store.invokeExtension("resources", "schedules.run", {
                    id: schedule.id,
                  });
                  await load();
                }).catch(store.setError.bind(store));
              }}
            >
              Run now
            </Button>
            <Button
              size="sm"
              variant="ghost"

              disabled={busy}
              onClick={() => {
                run(async () => {
                  await store.invokeExtension("resources", "schedules.put", {
                    ...schedule,
                    enabled: !schedule.enabled,
                  });
                  await load();
                }).catch(store.setError.bind(store));
              }}
            >
              {schedule.enabled ? "Pause" : "Enable"}
            </Button>
            <Button
              size="sm"
              variant="ghost"

              onClick={() => {
                setEditing(schedule);
                setText(schedule.text);
                const date = new Date(schedule.nextRunAtMs);
                setAt(
                  new Date(date.getTime() - date.getTimezoneOffset() * 60_000)
                    .toISOString()
                    .slice(0, 16),
                );
                setIntervalValue(
                  hasCount(schedule.intervalSeconds) ? String(schedule.intervalSeconds / 60) : "",
                );
              }}
            >
              Edit
            </Button>
            <Button
              size="sm"
              variant="destructive"

              disabled={busy}
              onClick={() => {
                run(async () => {
                  await store.invokeExtension("resources", "schedules.remove", {
                    id: schedule.id,
                  });
                  await load();
                }).catch(store.setError.bind(store));
              }}
            >
              Remove
            </Button>
          </div>
        </div>
      ))}
      <hr />
      <h3>{editing ? "Edit schedule" : "Schedule this conversation"}</h3>
      <label htmlFor={`${fieldPrefix}-field-1`}>
        Prompt
        <Textarea
          id={`${fieldPrefix}-field-1`}
          value={text}
          onChange={(e) => {
            setText(e.target.value);
          }}
        />
      </label>
      <Button
        size="sm"
        variant="ghost"
        onClick={() => {
          setText(thread.draft ?? "");
        }}
      >
        Use current draft
      </Button>
      <label htmlFor={`${fieldPrefix}-field-2`}>
        Next run
        <Input
          id={`${fieldPrefix}-field-2`}
          type="datetime-local"
          value={at}
          onChange={(e) => {
            setAt(e.target.value);
          }}
        />
      </label>
      <label htmlFor={`${fieldPrefix}-field-3`}>
        Repeat every (minutes)
        <Input
          id={`${fieldPrefix}-field-3`}
          type="number"
          min="1"
          value={intervalValue}
          placeholder="Leave empty for one run"
          onChange={(e) => {
            setIntervalValue(e.target.value);
          }}
        />
      </label>
      <Button
        variant="default"

        disabled={busy || !text.trim() || !at}
        onClick={() => {
          run(async () => {
            await store.saveSchedule(thread.id, {
              id: editing?.id,
              sessionId: editing?.sessionId,
              text,
              nextRunAtMs: new Date(at).getTime(),
              intervalSeconds: intervalValue ? Number(intervalValue) * 60 : undefined,
              enabled: true,
            });
            setEditing(undefined);
            setText("");
            setAt("");
            setIntervalValue("");
            await load();
          }).catch(store.setError.bind(store));
        }}
      >
        Save schedule
      </Button>
    </div>
  );
}
type Comparison = {
  id: string;
  status: string;
  createdAtMs: number;
  targets: {
    sessionId?: string;
    harness?: string;
    model?: string;
    status: string;
    text: string;
    error?: string;
    truncated?: boolean;
  }[];
};
function ComparisonOutput({
  comparison,
  onOpen,
}: {
  comparison: Comparison;
  onOpen: (sessionId: string) => void;
}) {
  return (
    <>
      {" "}
      {comparison.targets.map((target, i) => (
        <Disclosure key={target.sessionId ?? i} defaultOpen>
          <Disclosure.Summary>
            {target.harness ?? target.model ?? `Model ${i + 1}`} · {target.status}
          </Disclosure.Summary>
          <pre className="action-output">
            {target.text ||
              (target.error === undefined || target.error === ""
                ? "Waiting for output…"
                : target.error)}
          </pre>
          {target.truncated === true && (
            <p className="panel-note">
              Output truncated; open the conversation for its retained transcript.
            </p>
          )}
          {hasText(target.sessionId) && (
            <Button
              size="sm"
              variant="ghost"

              onClick={() => {
                if (hasText(target.sessionId)) {
                  onOpen(target.sessionId);
                }
              }}
            >
              Open conversation
            </Button>
          )}
        </Disclosure>
      ))}
    </>
  );
}
type ComparisonSummary = {
  id: string;
  status: string;
  createdAtMs: number;
  targets: {
    harness?: string;
    model?: string;
    sessionId?: string;
    status: string;
  }[];
};
function comparisonOptions(comparisons: ComparisonSummary[], comparisonId: string | undefined) {
  return [
    { id: "", label: "New comparison" },
    ...(hasText(comparisonId) && !comparisons.some((item) => item.id === comparisonId)
      ? [{ id: comparisonId, label: "Loading selected comparison…" }]
      : []),
    ...comparisons.map((item) => ({
      id: item.id,
      label: `${new Date(item.createdAtMs).toLocaleString()} · ${item.targets.map((target) => target.model ?? target.harness ?? "Model").join(" / ")} · ${item.status}`,
    })),
  ];
}
function comparisonTargets(thread: Thread) {
  return [
    { id: "first", harness: thread.harness, model: thread.model || "" },
    { id: "second", harness: thread.harness, model: "" },
  ];
}
function currentComparison(
  override: Comparison | undefined,
  selected: Comparison | undefined,
  id: string | undefined,
) {
  return override?.id === id ? override : selected;
}
export function Peers({ store, thread }: { store: WorkspaceStore; thread: Thread }) {
  const fieldPrefix = useId();

  const [targets, setTargets] = useState(() => comparisonTargets(thread));
  const [text, setText] = useState("");
  const [peerId, setPeerId] = useState("");
  const [peerText, setPeerText] = useState("");
  const [isolated, setIsolated] = useState(false);
  const [comparisonOverride, setComparisonOverride] = useState<Comparison>();
  const fetchComparison = async () =>
    await store.invokeExtension<Comparison>(
      "resources",
      "fanout.status",
      {
        id: thread.comparisonId,
      },
      extensionResult<Comparison>({
        type: "object",
        properties: {
          id: { type: "string" },
          status: { type: "string" },
          createdAtMs: { type: "number" },
          targets: {
            type: "array",
            items: {
              type: "object",
              properties: {
                sessionId: { type: "string" },
                harness: { type: "string" },
                model: { type: "string" },
                status: { type: "string" },
                text: { type: "string" },
                error: { type: "string" },
                truncated: { type: "boolean" },
              },
              required: ["status", "text"],
            },
          },
        },
        required: ["id", "status", "createdAtMs", "targets"],
      }),
    );
  const selected = useExtensionQuery(
    store,
    ["comparison", thread.comparisonId],
    fetchComparison,
    hasText(thread.comparisonId),
  );
  const comparison = currentComparison(comparisonOverride, selected.data, thread.comparisonId);
  const [busy, setBusy] = useState(false);
  const run = useCallback(
    async (action: () => Promise<unknown>) => {
      setBusy(true);
      try {
        await action();
      } catch (error) {
        store.setError(error);
      } finally {
        setBusy(false);
      }
    },
    [store],
  );
  const fetchPeers = useCallback(async () => {
    const [peers, retained] = await Promise.all([
      store.invokeExtension<{ sessions: SessionSnapshot[] }>(
        "resources",
        "peers.list",
        {},
        extensionResult<{ sessions: SessionSnapshot[] }>({
          type: "object",
          properties: { sessions: { type: "array", items: { $ref: "#/$defs/SessionSnapshot" } } },
          required: ["sessions"],
        }),
      ),
      store.invokeExtension<{ jobs: ComparisonSummary[] }>(
        "resources",
        "fanout.list",
        {
          limit: 128,
        },
        extensionResult<{ jobs: ComparisonSummary[] }>({
          type: "object",
          properties: {
            jobs: {
              type: "array",
              items: {
                type: "object",
                properties: {
                  id: { type: "string" },
                  status: { type: "string" },
                  createdAtMs: { type: "number" },
                  targets: {
                    type: "array",
                    items: {
                      type: "object",
                      properties: {
                        harness: { type: "string" },
                        model: { type: "string" },
                        sessionId: { type: "string" },
                        status: { type: "string" },
                      },
                      required: ["status"],
                    },
                  },
                },
                required: ["id", "status", "createdAtMs", "targets"],
              },
            },
          },
          required: ["jobs"],
        }),
      ),
    ]);
    return { sessions: peers.sessions, comparisons: retained.jobs };
  }, [store]);
  const catalog = useExtensionQuery(store, ["peers"], fetchPeers);
  const sessions = catalog.data?.sessions ?? [];
  const comparisons = catalog.data?.comparisons ?? [];
  const load = catalog.refresh;

  useEffect(() => {
    if (!comparison || !["running", "cancelling"].includes(comparison.status)) {
      return undefined;
    }
    let disposed = false;
    let pending = false;
    const timer = setInterval(() => {
      const handlePromiseFailure1 = (error: unknown) => {
        if (!disposed) {
          store.setError(error);
        }
      };

      if (pending) {
        return;
      }
      pending = true;
      store
        .invokeExtension<Comparison>(
          "resources",
          "fanout.status",
          {
            id: comparison.id,
          },
          extensionResult<Comparison>({
            type: "object",
            properties: {
              id: { type: "string" },
              status: { type: "string" },
              createdAtMs: { type: "number" },
              targets: {
                type: "array",
                items: {
                  type: "object",
                  properties: {
                    sessionId: { type: "string" },
                    harness: { type: "string" },
                    model: { type: "string" },
                    status: { type: "string" },
                    text: { type: "string" },
                    error: { type: "string" },
                    truncated: { type: "boolean" },
                  },
                  required: ["status", "text"],
                },
              },
            },
            required: ["id", "status", "createdAtMs", "targets"],
          }),
        )
        .then((result) => {
          if (!disposed) {
            setComparisonOverride(result);
          }
        })
        .catch(handlePromiseFailure1)
        .finally(() => {
          pending = false;
        })
        .catch(store.setError.bind(store));
    }, 1000);
    return () => {
      disposed = true;
      clearInterval(timer);
    };
  }, [comparison, store]);
  const openSession = async (sessionId: string) => {
    await store.restoreSessions();
    const found = store.getSnapshot().threads.find((t) => t.sessionId === sessionId);
    if (found) {
      await store.restoreThread(found.id);
      store.select(found.id);
    }
  };
  return (
    <div className="panel-body">
      <h3>Peer conversations</h3>
      <Button
        size="sm"
        variant="ghost"

        disabled={busy}
        onClick={() => {
          run(load).catch(store.setError.bind(store));
        }}
      >
        Refresh peers
      </Button>
      {sessions.map((session) => (
        <div className="resource-row" key={session.id}>
          <Button
            variant="command"

            onClick={() => {
              run(async () => {
                await openSession(session.id);
              }).catch(store.setError.bind(store));
            }}
          >
            {session.metadata?.title ?? session.id.slice(0, 8)}
            <span className="muted">
              {session.options.harness} · {session.state}
            </span>
          </Button>
          <Button
            size="sm"
            variant="ghost"
            onClick={() => {
              setPeerId(session.id);
            }}
          >
            Select
          </Button>
        </div>
      ))}
      <label htmlFor={`${fieldPrefix}-field-4`}>
        Selected conversation
        <Select
          id={`${fieldPrefix}-field-4`}
          label="Selected conversation"
          value={peerId}
          onChange={setPeerId}
          options={[
            { id: "", label: "New peer" },
            ...sessions.map((session) => ({
              id: session.id,
              label: session.metadata?.title ?? session.id,
            })),
          ]}
        />
      </label>
      <label htmlFor={`${fieldPrefix}-field-5`}>
        Instruction
        <Textarea
          id={`${fieldPrefix}-field-5`}
          value={peerText}
          onChange={(e) => {
            setPeerText(e.target.value);
          }}
        />
      </label>
      <div className="permission-actions">
        <Button
          size="sm"
          variant="ghost"

          disabled={busy || !peerText.trim()}
          onClick={() => {
            run(async () => {
              if (peerId) {
                await store.invokeExtension("resources", "peers.send", {
                  sessionId: peerId,
                  text: peerText,
                });
              } else {
                const result = await store.invokeExtension<{
                  session: SessionSnapshot;
                }>(
                  "resources",
                  "peers.spawn",
                  {
                    cwd: thread.worktreeId,
                    harness: thread.harness,
                    model: thread.model || undefined,
                    text: peerText,
                  },
                  extensionResult<{
                    session: SessionSnapshot;
                  }>({
                    type: "object",
                    properties: { session: { $ref: "#/$defs/SessionSnapshot" } },
                    required: ["session"],
                  }),
                );
                setPeerId(result.session.id);
              }
              setPeerText("");
              await load();
            }).catch(store.setError.bind(store));
          }}
        >
          {peerId ? "Send instruction" : "Start peer"}
        </Button>
      </div>
      <hr />
      <h3>Compare models</h3>
      <p className="panel-note">
        The same prompt runs in independent native conversations. Choose an agent tool and model for
        each target.
      </p>
      <label htmlFor={`${fieldPrefix}-field-6`}>
        Saved comparison
        <Select
          id={`${fieldPrefix}-field-6`}
          label="Saved comparison"
          value={thread.comparisonId ?? ""}
          disabled={busy}
          onChange={(value) => {
            store.updateThread(thread.id, { comparisonId: value || undefined });
          }}
          options={comparisonOptions(comparisons, thread.comparisonId)}
        />
      </label>
      <p className="panel-note">
        Results are retained by the daemon. Removing a comparison keeps its conversations and
        transcripts.
      </p>
      {targets.map((target, index) => (
        <div className="flex flex-col gap-2" key={target.id}>
          <label htmlFor={`${fieldPrefix}-field-7-${target.id}`}>
            Agent tool {index + 1}
            <HarnessSelect
              id={`${fieldPrefix}-field-7-${target.id}`}
              label={`Comparison agent ${index + 1}`}
              value={target.harness}
              disabled={busy}
              harnesses={store.getSnapshot().harnesses}
              disabledHarnesses={store.getSnapshot().disabledHarnesses}
              onChange={(harness) => {
                setTargets((items) =>
                  items.map((item) => (item.id === target.id ? { ...item, harness } : item)),
                );
              }}
            />
          </label>
          <label htmlFor={`${fieldPrefix}-field-8-${target.id}`}>
            Model {index + 1}
            <Input
              id={`${fieldPrefix}-field-8-${target.id}`}
              aria-label={`Comparison model ${index + 1}`}
              placeholder="Default model"
              value={target.model}
              disabled={busy}
              onChange={(event) => {
                setTargets((items) =>
                  items.map((item) =>
                    item.id === target.id ? { ...item, model: event.target.value } : item,
                  ),
                );
              }}
            />
          </label>
          <Button
            variant="ghost"
            disabled={busy || targets.length <= 2}
            onClick={() => {
              setTargets((items) => items.filter((item) => item.id !== target.id));
            }}
          >
            Remove target
          </Button>
        </div>
      ))}
      <Button
        variant="ghost"
        disabled={busy || targets.length >= 8}
        onClick={() => {
          setTargets((items) => [
            ...items,
            { id: crypto.randomUUID(), harness: thread.harness, model: "" },
          ]);
        }}
      >
        Add target
      </Button>
      <label htmlFor={`${fieldPrefix}-field-9`}>
        Task
        <Textarea
          id={`${fieldPrefix}-field-9`}
          value={text}
          onChange={(e) => {
            setText(e.target.value);
          }}
        />
      </label>
      <Button
        size="sm"
        variant="ghost"
        onClick={() => {
          setText(thread.draft ?? "");
        }}
      >
        Use current draft
      </Button>
      {store.getSnapshot().projects.find((p) => p.id === thread.projectId)?.git === true && (
        <Checkbox
          label="Use a separate worktree for each model"
          checked={isolated}
          onChange={setIsolated}
        />
      )}
      <Button
        variant="default"

        disabled={
          busy ||
          targets.length < 2 ||
          targets.length > 8 ||
          targets.some((target) =>
            store.getSnapshot().disabledHarnesses.includes(target.harness ?? thread.harness),
          ) ||
          !text.trim() ||
          (!!comparison && ["running", "cancelling"].includes(comparison.status))
        }
        onClick={() => {
          run(async () => {
            if (
              targets.some((target) =>
                store.getSnapshot().disabledHarnesses.includes(target.harness ?? thread.harness),
              )
            ) {
              throw new Error(
                "Enable the selected agent tools in Settings before starting a comparison.",
              );
            }
            const rawComparison = await store.rpc("client/comparison", {
              launch: await store.sessionIntent(thread.id),
              text,
              isolated,
              targets: targets.map((target) => ({
                harness: target.harness,
                model: target.model.trim() || undefined,
              })),
            });
            const result = extensionResult<Comparison>({
              type: "object",
              properties: {
                id: { type: "string" },
                status: { type: "string" },
                createdAtMs: { type: "number" },
                targets: {
                  type: "array",
                  items: {
                    type: "object",
                    properties: {
                      sessionId: { type: "string" },
                      harness: { type: "string" },
                      model: { type: "string" },
                      status: { type: "string" },
                      text: { type: "string" },
                      error: { type: "string" },
                      truncated: { type: "boolean" },
                    },
                    required: ["status", "text"],
                  },
                },
              },
              required: ["id", "status", "createdAtMs", "targets"],
            })(rawComparison);
            setComparisonOverride(result);
            store.updateThread(thread.id, { comparisonId: result.id });
            await load();
          }).catch(store.setError.bind(store));
        }}
      >
        Start comparison
      </Button>
      {comparison && (
        <>
          <div className="review-toolbar">
            <strong>{comparison.status}</strong>
            <Button
              size="sm"
              variant="destructive"

              disabled={busy || ["running", "cancelling"].includes(comparison.status)}
              onClick={() => {
                run(async () => {
                  await store.invokeExtension("resources", "fanout.remove", {
                    id: comparison.id,
                  });
                  setComparisonOverride(undefined);
                  store.updateThread(thread.id, { comparisonId: undefined });
                  await load();
                }).catch(store.setError.bind(store));
              }}
            >
              Remove comparison
            </Button>
            {comparison.status === "running" && (
              <Button
                size="sm"
                variant="ghost"

                onClick={() => {
                  run(async () => {
                    const result = await store.invokeExtension<{
                      job: Comparison;
                      errors: unknown[];
                    }>(
                      "resources",
                      "fanout.cancel",
                      { id: comparison.id },
                      extensionResult<{
                        job: Comparison;
                        errors: unknown[];
                      }>({
                        type: "object",
                        properties: {
                          job: {
                            type: "object",
                            properties: {
                              id: { type: "string" },
                              status: { type: "string" },
                              createdAtMs: { type: "number" },
                              targets: {
                                type: "array",
                                items: {
                                  type: "object",
                                  properties: {
                                    sessionId: { type: "string" },
                                    harness: { type: "string" },
                                    model: { type: "string" },
                                    status: { type: "string" },
                                    text: { type: "string" },
                                    error: { type: "string" },
                                    truncated: { type: "boolean" },
                                  },
                                  required: ["status", "text"],
                                },
                              },
                            },
                            required: ["id", "status", "createdAtMs", "targets"],
                          },
                          errors: { type: "array", items: {} },
                        },
                        required: ["job", "errors"],
                      }),
                    );
                    setComparisonOverride(result.job);
                    if (result.errors.length) {
                      store.setError(
                        `Some conversations could not be cancelled: ${JSON.stringify(result.errors)}`,
                      );
                    }
                  }).catch(store.setError.bind(store));
                }}
              >
                Cancel comparison
              </Button>
            )}
          </div>
          <ComparisonOutput
            comparison={comparison}
            onOpen={(sessionId) => {
              run(async () => {
                await openSession(sessionId);
              }).catch(store.setError.bind(store));
            }}
          />
        </>
      )}
    </div>
  );
}

type UsageTotals = {
  inputTokens: number;
  outputTokens: number;
  cachedInputTokens: number;
  cacheWriteTokens: number;
  totalTokens: number;
  knownCostUsd: number;
  unknownCosts: number;
  unknownTokenFields: number;
  observations: number;
};
type UsageReport = {
  totals: UsageTotals;
  byDay: Record<string, UsageTotals>;
  byHarness: Record<string, UsageTotals>;
  byModel: Record<string, UsageTotals>;
  truncated: boolean;
};
function UsageTable({ rows }: { rows: Record<string, UsageTotals> }) {
  return (
    <div className="usage-table">
      <table>
        <thead>
          <tr>
            <th>Group</th>
            <th>Input</th>
            <th>Output</th>
            <th>Cached input</th>
            <th>Reported cost</th>
            <th>Unknown cost reports</th>
          </tr>
        </thead>
        <tbody>
          {Object.entries(rows).map(([name, value]) => (
            <tr key={name}>
              <th>{name}</th>
              <td>{value.inputTokens.toLocaleString()}</td>
              <td>{value.outputTokens.toLocaleString()}</td>
              <td>{value.cachedInputTokens.toLocaleString()}</td>
              <td>
                {value.observations === value.unknownCosts
                  ? "Unknown"
                  : `$${value.knownCostUsd.toFixed(4)}`}
              </td>
              <td>{value.unknownCosts.toLocaleString()}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
export function UsageDashboard({ store, thread }: { store: WorkspaceStore; thread: Thread }) {
  const [quotas, setQuotas] = useState<unknown>();
  const [all, setAll] = useState(false);
  const [busy, setBusy] = useState(false);
  const fetchUsage = async () => {
    const result = await store.invokeExtension<UsageReport>(
      "resources",
      "usage.dashboard",
      {
        cwd: all ? undefined : thread.worktreeId,
      },
      extensionResult<UsageReport>({
        type: "object",
        properties: {
          totals: {
            type: "object",
            properties: {
              inputTokens: { type: "number" },
              outputTokens: { type: "number" },
              cachedInputTokens: { type: "number" },
              cacheWriteTokens: { type: "number" },
              totalTokens: { type: "number" },
              knownCostUsd: { type: "number" },
              unknownCosts: { type: "number" },
              unknownTokenFields: { type: "number" },
              observations: { type: "number" },
            },
            required: [
              "inputTokens",
              "outputTokens",
              "cachedInputTokens",
              "cacheWriteTokens",
              "totalTokens",
              "knownCostUsd",
              "unknownCosts",
              "unknownTokenFields",
              "observations",
            ],
          },
          byDay: {
            type: "object",
            additionalProperties: {
              type: "object",
              properties: {
                inputTokens: { type: "number" },
                outputTokens: { type: "number" },
                cachedInputTokens: { type: "number" },
                cacheWriteTokens: { type: "number" },
                totalTokens: { type: "number" },
                knownCostUsd: { type: "number" },
                unknownCosts: { type: "number" },
                unknownTokenFields: { type: "number" },
                observations: { type: "number" },
              },
              required: [
                "inputTokens",
                "outputTokens",
                "cachedInputTokens",
                "cacheWriteTokens",
                "totalTokens",
                "knownCostUsd",
                "unknownCosts",
                "unknownTokenFields",
                "observations",
              ],
            },
          },
          byHarness: {
            type: "object",
            additionalProperties: {
              type: "object",
              properties: {
                inputTokens: { type: "number" },
                outputTokens: { type: "number" },
                cachedInputTokens: { type: "number" },
                cacheWriteTokens: { type: "number" },
                totalTokens: { type: "number" },
                knownCostUsd: { type: "number" },
                unknownCosts: { type: "number" },
                unknownTokenFields: { type: "number" },
                observations: { type: "number" },
              },
              required: [
                "inputTokens",
                "outputTokens",
                "cachedInputTokens",
                "cacheWriteTokens",
                "totalTokens",
                "knownCostUsd",
                "unknownCosts",
                "unknownTokenFields",
                "observations",
              ],
            },
          },
          byModel: {
            type: "object",
            additionalProperties: {
              type: "object",
              properties: {
                inputTokens: { type: "number" },
                outputTokens: { type: "number" },
                cachedInputTokens: { type: "number" },
                cacheWriteTokens: { type: "number" },
                totalTokens: { type: "number" },
                knownCostUsd: { type: "number" },
                unknownCosts: { type: "number" },
                unknownTokenFields: { type: "number" },
                observations: { type: "number" },
              },
              required: [
                "inputTokens",
                "outputTokens",
                "cachedInputTokens",
                "cacheWriteTokens",
                "totalTokens",
                "knownCostUsd",
                "unknownCosts",
                "unknownTokenFields",
                "observations",
              ],
            },
          },
          truncated: { type: "boolean" },
        },
        required: ["totals", "byDay", "byHarness", "byModel", "truncated"],
      }),
    );
    return result;
  };
  const catalog = useExtensionQuery(store, ["usage-dashboard", all, thread.worktreeId], fetchUsage);
  const usage = catalog.data;
  const load = catalog.refresh;

  return (
    <div className="panel-body">
      <h3>Observed usage</h3>
      <Checkbox label="All projects" checked={all} onChange={setAll} />
      <Button
        size="sm"
        variant="ghost"
        disabled={busy}
        onClick={() => {
          load().catch(store.setError.bind(store));
        }}
      >
        Refresh usage
      </Button>
      <p className="panel-note">
        Reported token counts and costs. Missing fields remain unknown; subscription bills are
        separate.
      </p>
      {usage && (
        <>
          {usage.totals.observations === 0 ? (
            <p className="panel-note">No native usage reports have been recorded in this scope.</p>
          ) : (
            <>
              <UsageTable rows={{ Total: usage.totals }} />
              <p className="panel-note">
                {usage.totals.observations.toLocaleString()} reports ·{" "}
                {usage.totals.unknownTokenFields.toLocaleString()} unknown token fields. Displayed
                counts sum the known fields.
              </p>
              {(
                [
                  ["By day", usage.byDay],
                  ["By agent tool", usage.byHarness],
                  ["By model", usage.byModel],
                ] as const
              ).map(([title, rows]) => (
                <Disclosure key={title}>
                  <Disclosure.Summary>{title}</Disclosure.Summary>
                  <UsageTable rows={rows} />
                </Disclosure>
              ))}
            </>
          )}
          {usage.truncated && (
            <p className="panel-note">
              The retained ledger has reached its limit. Older observations are excluded.
            </p>
          )}
        </>
      )}
      <hr />
      <h3>Native account quotas</h3>
      <Button
        size="sm"
        variant="ghost"

        disabled={busy || !thread.worktreeId}
        onClick={() => {
          setBusy(true);
          const query = {
            harness: thread.harness,
            cwd: thread.worktreeId,
            config: thread.config ?? {},
          };
          const request =
            hasText(thread.sessionId) && !["closed", "failed"].includes(thread.state)
              ? store.rpc("agent/quotas", { sessionId: thread.sessionId })
              : store.rpc("runtime/quotas", {
                  harness: query.harness,
                  cwd: query.cwd,
                  config: query.config,
                });
          request
            .then(setQuotas)
            .catch(store.setError.bind(store))
            .finally(() => {
              setBusy(false);
            })
            .catch(store.setError.bind(store));
        }}
      >
        Read {thread.harness} quotas
      </Button>
      {quotas !== undefined && (
        <pre className="action-output">{JSON.stringify(quotas, null, 2)}</pre>
      )}
    </div>
  );
}
