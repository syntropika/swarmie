import { Disclosure } from "@/components/ui/disclosure";
import { useEffect, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { Select } from "@/components/ui/select";
import { HarnessSelect } from "./HarnessSelect";
import { Checkbox } from "@/components/ui/checkbox";
import type { WorkspaceStore } from "./workspace";
import type { SessionSnapshot, Thread } from "./protocol";
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
export function Schedules({
  store,
  thread,
}: {
  store: WorkspaceStore;
  thread: Thread;
}) {
  const [schedules, setSchedules] = useState<Schedule[]>([]);
  const [text, setText] = useState("");
  const [at, setAt] = useState("");
  const [interval, setIntervalValue] = useState("");
  const [editing, setEditing] = useState<Schedule>();
  const [busy, setBusy] = useState(false);
  const run = async (action: () => Promise<unknown>) => {
    setBusy(true);
    try {
      await action();
    } catch (e) {
      store.setError(e);
    } finally {
      setBusy(false);
    }
  };
  const load = async () =>
    setSchedules(
      (
        await store.invokeExtension<{ schedules: Schedule[] }>(
          "resources",
          "schedules.list",
          {},
        )
      ).schedules,
    );
  useEffect(() => {
    void run(load);
  }, []);
  return (
    <div className="panel-body">
      <h3>Scheduled prompts</h3>
      <p className="panel-note">
        Runs while the daemon is active. Busy conversations receive a queued
        prompt; permissions stay interactive.
      </p>
      <Button
        variant="ghost"
        className="small-button"
        disabled={busy}
        onClick={() => void run(load)}
      >
        Refresh schedules
      </Button>
      {schedules.map((schedule) => (
        <div className="schedule-row" key={schedule.id}>
          <strong>{schedule.text.slice(0, 100)}</strong>
          <p className="muted">
            {schedule.status} ·{" "}
            {new Date(schedule.nextRunAtMs).toLocaleString()}
            {schedule.intervalSeconds
              ? ` · every ${schedule.intervalSeconds / 60} minutes`
              : ""}
          </p>
          {schedule.error && <p className="inline-error">{schedule.error}</p>}
          <div className="permission-actions">
            <Button
              variant="ghost"
              className="small-button"
              disabled={busy}
              onClick={() =>
                void run(async () => {
                  await store.invokeExtension("resources", "schedules.run", {
                    id: schedule.id,
                  });
                  await load();
                })
              }
            >
              Run now
            </Button>
            <Button
              variant="ghost"
              className="small-button"
              disabled={busy}
              onClick={() =>
                void run(async () => {
                  await store.invokeExtension("resources", "schedules.put", {
                    ...schedule,
                    enabled: !schedule.enabled,
                  });
                  await load();
                })
              }
            >
              {schedule.enabled ? "Pause" : "Enable"}
            </Button>
            <Button
              variant="ghost"
              className="small-button"
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
                  schedule.intervalSeconds
                    ? String(schedule.intervalSeconds / 60)
                    : "",
                );
              }}
            >
              Edit
            </Button>
            <Button
              variant="ghost"
              className="small-button danger-button"
              disabled={busy}
              onClick={() =>
                void run(async () => {
                  await store.invokeExtension("resources", "schedules.remove", {
                    id: schedule.id,
                  });
                  await load();
                })
              }
            >
              Remove
            </Button>
          </div>
        </div>
      ))}
      <hr />
      <h3>{editing ? "Edit schedule" : "Schedule this conversation"}</h3>
      <label>
        Prompt
        <Textarea value={text} onChange={(e) => setText(e.target.value)} />
      </label>
      <Button
        variant="ghost"
        className="small-button"
        onClick={() => setText(thread.draft ?? "")}
      >
        Use current draft
      </Button>
      <label>
        Next run
        <Input
          type="datetime-local"
          value={at}
          onChange={(e) => setAt(e.target.value)}
        />
      </label>
      <label>
        Repeat every (minutes)
        <Input
          type="number"
          min="1"
          value={interval}
          placeholder="Leave empty for one run"
          onChange={(e) => setIntervalValue(e.target.value)}
        />
      </label>
      <Button
        variant="ghost"
        className="primary-button"
        disabled={busy || !text.trim() || !at}
        onClick={() =>
          void run(async () => {
            await store.saveSchedule(thread.id, {
              id: editing?.id, sessionId: editing?.sessionId, text,
              nextRunAtMs: new Date(at).getTime(), intervalSeconds: interval ? Number(interval) * 60 : undefined, enabled: true,
            });
            setEditing(undefined);
            setText("");
            setAt("");
            setIntervalValue("");
            await load();
          })
        }
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
export function Peers({
  store,
  thread,
}: {
  store: WorkspaceStore;
  thread: Thread;
}) {
  const [sessions, setSessions] = useState<SessionSnapshot[]>([]);
  const [targets, setTargets] = useState([
    { id: "first", harness: thread.harness, model: thread.model || "" },
    { id: "second", harness: thread.harness, model: "" },
  ]);
  const [text, setText] = useState("");
  const [peerId, setPeerId] = useState("");
  const [peerText, setPeerText] = useState("");
  const [isolated, setIsolated] = useState(false);
  const [comparison, setComparison] = useState<Comparison>();
  const [comparisons, setComparisons] = useState<ComparisonSummary[]>([]);
  useEffect(() => {
    setComparison(undefined);
    if (!thread.comparisonId) return;
    let disposed = false;
    void store
      .invokeExtension<Comparison>("resources", "fanout.status", {
        id: thread.comparisonId,
      })
      .then((result) => {
        if (!disposed) setComparison(result);
      })
      .catch((error) => {
        if (!disposed) store.setError(error);
      });
    return () => {
      disposed = true;
    };
  }, [thread.comparisonId]);
  const [busy, setBusy] = useState(false);
  const run = async (action: () => Promise<unknown>) => {
    setBusy(true);
    try {
      await action();
    } catch (e) {
      store.setError(e);
    } finally {
      setBusy(false);
    }
  };
  const load = async () => {
    const [peers, retained] = await Promise.all([
      store.invokeExtension<{ sessions: SessionSnapshot[] }>(
        "resources",
        "peers.list",
        {},
      ),
      store.invokeExtension<{ jobs: ComparisonSummary[] }>(
        "resources",
        "fanout.list",
        { limit: 128 },
      ),
    ]);
    setComparisons(retained.jobs);
    setSessions(peers.sessions);
  };
  useEffect(() => {
    void run(load);
  }, []);
  useEffect(() => {
    if (!comparison || !["running", "cancelling"].includes(comparison.status))
      return;
    let disposed = false,
      pending = false;
    const timer = setInterval(() => {
      if (pending) return;
      pending = true;
      void store
        .invokeExtension<Comparison>("resources", "fanout.status", {
          id: comparison.id,
        })
        .then((result) => {
          if (!disposed) {
            setComparison(result);
            setComparisons((items) =>
              items.map((item) =>
                item.id === result.id
                  ? { ...item, status: result.status }
                  : item,
              ),
            );
          }
        })
        .catch((e) => {
          if (!disposed) store.setError(e);
        })
        .finally(() => {
          pending = false;
        });
    }, 1000);
    return () => {
      disposed = true;
      clearInterval(timer);
    };
  }, [comparison?.id, comparison?.status]);
  const openSession = async (sessionId: string) => {
    await store.restoreSessions();
    const found = store
      .getSnapshot()
      .threads.find((t) => t.sessionId === sessionId);
    if (found) {
      await store.restoreThread(found.id);
      store.select(found.id);
    }
  };
  return (
    <div className="panel-body">
      <h3>Peer conversations</h3>
      <Button
        variant="ghost"
        className="small-button"
        disabled={busy}
        onClick={() => void run(load)}
      >
        Refresh peers
      </Button>
      {sessions.map((session) => (
        <div className="resource-row" key={session.id}>
          <Button
            variant="ghost"
            className="command-result"
            onClick={() => void run(() => openSession(session.id))}
          >
            {session.metadata?.title ?? session.id.slice(0, 8)}
            <span className="muted">
              {session.options.harness} · {session.state}
            </span>
          </Button>
          <Button
            variant="ghost"
            className="small-button"
            onClick={() => setPeerId(session.id)}
          >
            Select
          </Button>
        </div>
      ))}
      <label>
        Selected conversation
        <Select label="Selected conversation" value={peerId} onChange={setPeerId} options={[{ id: "", label: "New peer" }, ...sessions.map((session) => ({ id: session.id, label: session.metadata?.title ?? session.id }))]} />
      </label>
      <label>
        Instruction
        <Textarea
          value={peerText}
          onChange={(e) => setPeerText(e.target.value)}
        />
      </label>
      <div className="permission-actions">
        <Button
          variant="ghost"
          className="small-button"
          disabled={busy || !peerText.trim()}
          onClick={() =>
            void run(async () => {
              if (peerId)
                await store.invokeExtension("resources", "peers.send", {
                  sessionId: peerId,
                  text: peerText,
                });
              else {
                const result = await store.invokeExtension<{
                  session: SessionSnapshot;
                }>("resources", "peers.spawn", {
                  cwd: thread.worktreeId,
                  harness: thread.harness,
                  model: thread.model || undefined,
                  text: peerText,
                });
                setPeerId(result.session.id);
              }
              setPeerText("");
              await load();
            })
          }
        >
          {peerId ? "Send instruction" : "Start peer"}
        </Button>
      </div>
      <hr />
      <h3>Compare models</h3>
      <p className="panel-note">
        The same prompt runs in independent native conversations. Choose
        a harness and model for each target.
      </p>
      <label>
        Saved comparison
        <Select label="Saved comparison" value={thread.comparisonId ?? ""} disabled={busy}
          onChange={(value) => store.updateThread(thread.id, { comparisonId: value || undefined })}
          options={[{ id: "", label: "New comparison" },
            ...(thread.comparisonId && !comparisons.some((item) => item.id === thread.comparisonId) ? [{ id: thread.comparisonId, label: "Loading selected comparison…" }] : []),
            ...comparisons.map((item) => ({ id: item.id, label: `${new Date(item.createdAtMs).toLocaleString()} · ${item.targets.map((target) => target.model ?? target.harness ?? "Model").join(" / ")} · ${item.status}` }))]} />
      </label>
      <p className="panel-note">
        Results are retained by the daemon. Removing a comparison keeps its
        conversations and transcripts.
      </p>
      {targets.map((target, index) => (
        <div className="comparison-target" key={target.id}>
          <label>Harness {index + 1}<HarnessSelect label={`Comparison harness ${index + 1}`} value={target.harness} disabled={busy}
            harnesses={store.getSnapshot().harnesses}
            onChange={(harness) => setTargets((items) => items.map((item) => item.id === target.id ? { ...item, harness } : item))} /></label>
          <label>Model {index + 1}<Input aria-label={`Comparison model ${index + 1}`} placeholder="Default model" value={target.model} disabled={busy}
            onChange={(event) => setTargets((items) => items.map((item) => item.id === target.id ? { ...item, model: event.target.value } : item))} /></label>
          <Button variant="ghost" disabled={busy || targets.length <= 2} onClick={() => setTargets((items) => items.filter((item) => item.id !== target.id))}>Remove target</Button>
        </div>
      ))}
      <Button variant="ghost" disabled={busy || targets.length >= 8} onClick={() => setTargets((items) => [...items, { id: crypto.randomUUID(), harness: thread.harness, model: "" }])}>Add target</Button>
      <label>
        Task
        <Textarea value={text} onChange={(e) => setText(e.target.value)} />
      </label>
      <Button
        variant="ghost"
        className="small-button"
        onClick={() => setText(thread.draft ?? "")}
      >
        Use current draft
      </Button>
      {store.getSnapshot().projects.find((p) => p.id === thread.projectId)
        ?.git && (
        <Checkbox label="Use a separate worktree for each model" checked={isolated} onChange={setIsolated} />
      )}
      <Button
        variant="ghost"
        className="primary-button"
        disabled={
          busy ||
          targets.length < 2 ||
          targets.length > 8 ||
          !text.trim() ||
          (!!comparison &&
            ["running", "cancelling"].includes(comparison.status))
        }
        onClick={() =>
          void run(async () => {
            const result = await store.rpc<Comparison>("client/comparison", { launch: await store.sessionIntent(thread.id), text, isolated, targets: targets.map(target => ({ harness: target.harness, model: target.model.trim() || undefined })) });
            setComparison(result);
            store.updateThread(thread.id, { comparisonId: result.id });
            await load();
          })
        }
      >
        Start comparison
      </Button>
      {comparison && (
        <>
          <div className="review-toolbar">
            <strong>{comparison.status}</strong>
            <Button
              variant="ghost"
              className="small-button danger-button"
              disabled={
                busy || ["running", "cancelling"].includes(comparison.status)
              }
              onClick={() =>
                void run(async () => {
                  await store.invokeExtension("resources", "fanout.remove", {
                    id: comparison.id,
                  });
                  setComparison(undefined);
                  store.updateThread(thread.id, { comparisonId: undefined });
                  await load();
                })
              }
            >
              Remove comparison
            </Button>
            {comparison.status === "running" && (
              <Button
                variant="ghost"
                className="small-button"
                onClick={() =>
                  void run(async () => {
                    const result = await store.invokeExtension<{
                      job: Comparison;
                      errors: unknown[];
                    }>("resources", "fanout.cancel", { id: comparison.id });
                    setComparison(result.job);
                    if (result.errors.length)
                      store.setError(
                        `Some conversations could not be cancelled: ${JSON.stringify(result.errors)}`,
                      );
                  })
                }
              >
                Cancel comparison
              </Button>
            )}
          </div>
          {comparison.targets.map((target, i) => (
            <Disclosure key={target.sessionId ?? i} defaultOpen>
              <Disclosure.Summary>
                {target.harness ?? target.model ?? `Model ${i + 1}`} ·{" "}
                {target.status}
              </Disclosure.Summary>
              <pre className="action-output">
                {target.text || target.error || "Waiting for output…"}
              </pre>
              {target.truncated && (
                <p className="panel-note">
                  Output truncated; open the conversation for its retained
                  transcript.
                </p>
              )}
              {target.sessionId && (
                <Button
                  variant="ghost"
                  className="small-button"
                  onClick={() => void run(() => openSession(target.sessionId!))}
                >
                  Open conversation
                </Button>
              )}
            </Disclosure>
          ))}
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
export function UsageDashboard({
  store,
  thread,
}: {
  store: WorkspaceStore;
  thread: Thread;
}) {
  const [usage, setUsage] = useState<UsageReport>();
  const [quotas, setQuotas] = useState<unknown>();
  const [all, setAll] = useState(false);
  const [busy, setBusy] = useState(false);
  const revision = useRef(0);
  const load = async () => {
    const request = ++revision.current;
    setBusy(true);
    try {
      const result = await store.invokeExtension<UsageReport>(
        "resources",
        "usage.dashboard",
        { cwd: all ? undefined : thread.worktreeId },
      );
      if (request === revision.current) setUsage(result);
    } catch (error) {
      if (request === revision.current) store.setError(error);
    } finally {
      if (request === revision.current) setBusy(false);
    }
  };
  useEffect(() => {
    void load();
    return () => {
      revision.current++;
    };
  }, [all, thread.worktreeId]);
  return (
    <div className="panel-body">
      <h3>Observed usage</h3>
      <Checkbox label="All projects" checked={all} onChange={setAll} />
      <Button
        variant="ghost"
        className="small-button"
        disabled={busy}
        onClick={() => void load()}
      >
        Refresh usage
      </Button>
      <p className="panel-note">
        Reported token counts and costs. Missing fields remain unknown;
        subscription bills are separate.
      </p>
      {usage && (
        <>
          {usage.totals.observations === 0 ? (
            <p className="panel-note">
              No native usage reports have been recorded in this scope.
            </p>
          ) : (
            <>
              <UsageTable rows={{ Total: usage.totals }} />
              <p className="panel-note">
                {usage.totals.observations.toLocaleString()} reports ·{" "}
                {usage.totals.unknownTokenFields.toLocaleString()} unknown token
                fields. Displayed counts sum the known fields.
              </p>
              {(
                [
                  ["By day", usage.byDay],
                  ["By harness", usage.byHarness],
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
              The retained ledger has reached its limit. Older observations are
              excluded.
            </p>
          )}
        </>
      )}
      <hr />
      <h3>Native account quotas</h3>
      <Button
        variant="ghost"
        className="small-button"
        disabled={busy || !thread.worktreeId}
        onClick={() => {
          setBusy(true);
          const query = {
            harness: thread.harness,
            cwd: thread.worktreeId,
            config: thread.config ?? {},
          };
          const request =
            thread.sessionId && !["closed", "failed"].includes(thread.state)
              ? store.rpc("agent/quotas", { sessionId: thread.sessionId })
              : store.rpc("runtime/quotas", {
                    harness: query.harness,
                    cwd: query.cwd,
                    config: query.config,
                  });
          void request
            .then(setQuotas)
            .catch((error) => store.setError(error))
            .finally(() => setBusy(false));
        }}
      >
        Read {thread.harness} quotas
      </Button>
      {quotas != null && (
        <pre className="action-output">{JSON.stringify(quotas, null, 2)}</pre>
      )}
    </div>
  );
}
