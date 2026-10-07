import { ComboBox } from "@/components/ui/combobox";
import { Disclosure } from "@/components/ui/disclosure";
import { useEffect, useRef, useState } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { createTerminalInput } from "./terminal-input";
import "@xterm/xterm/css/xterm.css";
import { Button } from "@/components/ui/button";
import { Select } from "@/components/ui/select";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import type { ExtensionInfo, ExtensionView, Thread, Review } from "./protocol";
import { fingerprint, reviewKey } from "./reviewed";
import type { WorkspaceStore } from "./workspace";
import { ResourceLibrary } from "./ResourceLibrary";
import { ProjectSettings } from "./ProjectSettings";
import { Schedules, Peers, UsageDashboard } from "./WorkflowViews";
import { CustomExtensionView } from "./CustomExtensionView";
import { BrowserPreview, Devices, SystemWindows } from "./PlatformViews";
import { GitWorkflow } from "./GitWorkflow";

type Host = {
  store: WorkspaceStore;
  thread: Thread;
  extensionId: string;
  view: ExtensionView;
};
function Files({ store, thread, extensionId, view }: Host) {
  const editor = useRef<HTMLTextAreaElement>(null);
  const [path, setPath] = useState("");
  const [entries, setEntries] = useState<
    { name: string; path: string; kind: string }[]
  >([]);
  const [file, setFile] = useState("");
  const [content, setContent] = useState("");
  const [dirty, setDirty] = useState(false);
  const [saving, setSaving] = useState(false);
  const requests = useRef({ list: 0, file: 0 });
  const currentThread = () =>
    store.getSnapshot().threads.find((t) => t.id === thread.id)!;
  const args = { projectId: thread.worktreeId };
  const load = async (next: string) => {
    const request = ++requests.current.list;
    try {
      const result = await store.invokeExtension<{ entries: typeof entries }>(
        extensionId,
        view.command,
        { ...args, path: next },
      );
      if (request !== requests.current.list) return;
      setPath(next);
      setEntries(result.entries);
    } catch (error) {
      store.setError(error);
    }
  };
  const open = async (next: string, reload = false) => {
    const request = ++requests.current.file;
    try {
      await store.ready();
      if (request !== requests.current.file) return;
      const draft = currentThread().fileDrafts?.[next];
      if (draft && !reload) {
        setFile(next);
        setContent(draft.content);
        setDirty(draft.content !== draft.original);
        return;
      }
      const result = await store.invokeExtension<{ content: string }>(
        extensionId,
        "files.read",
        { ...args, path: next },
      );
      if (request !== requests.current.file) return;
      setFile(next);
      setContent(result.content);
      setDirty(false);
      store.updateThread(thread.id, {
        fileDrafts: {
          ...currentThread().fileDrafts,
          [next]: { content: result.content, original: result.content },
        },
      });
    } catch (error) {
      store.setError(error);
    }
  };
  useEffect(() => {
    void load("");
    return () => {
      requests.current.list++;
      requests.current.file++;
    };
  }, [thread.worktreeId]);
  useEffect(() => {
    const location = thread.layout?.activeFile;
    if (location) void open(location.path);
    else {
      requests.current.file++;
      setFile("");
      setDirty(false);
    }
  }, [thread.layout?.activeFile?.path, thread.worktreeId]);
  useEffect(() => {
    const location = thread.layout?.activeFile;
    if (!editor.current || file !== location?.path || !location.startLine)
      return;
    const lines = content.split("\n");
    const start =
      lines.slice(0, location.startLine - 1).join("\n").length +
      (location.startLine > 1 ? 1 : 0);
    const end = lines
      .slice(0, location.endLine ?? location.startLine)
      .join("\n").length;
    editor.current.focus();
    editor.current.setSelectionRange(start, end);
    editor.current.scrollTop = Math.max(0, (location.startLine - 3) * 20);
  }, [
    file,
    thread.layout?.activeFile?.startLine,
    thread.layout?.activeFile?.endLine,
  ]);
  const close = (closed: string) => {
    const current = currentThread();
    const draft = current.fileDrafts?.[closed];
    if (
      draft &&
      draft.content !== draft.original &&
      !window.confirm(`Discard the unsaved draft for ${closed}?`)
    )
      return;
    const files =
      current.layout?.files?.filter((tab) => tab.path !== closed) ?? [];
    const drafts = { ...current.fileDrafts };
    delete drafts[closed];
    store.updateThread(thread.id, {
      fileDrafts: drafts,
      layout: {
        ...current.layout,
        files,
        activeFile:
          current.layout?.activeFile?.path === closed
            ? files.at(-1)
            : current.layout?.activeFile,
      },
    });
  };
  const save = async () => {
    setSaving(true);
    const savedPath = file,
      savedContent = content;
    try {
      const baseline = currentThread().fileDrafts?.[savedPath]?.original;
      const latest = await store.invokeExtension<{ content: string }>(
        extensionId,
        "files.read",
        { ...args, path: savedPath },
      );
      if (latest.content !== baseline)
        throw new Error(
          "This file changed on disk. Copy your draft or reload the file before saving.",
        );
      await store.invokeExtension(extensionId, "files.write", {
        ...args,
        path: savedPath,
        content: savedContent,
      });
      const current = currentThread();
      const remaining =
        current.fileDrafts?.[savedPath]?.content ?? savedContent;
      store.updateThread(thread.id, {
        fileDrafts: {
          ...current.fileDrafts,
          [savedPath]: { content: remaining, original: savedContent },
        },
      });
      if (current.layout?.activeFile?.path === savedPath)
        setDirty(remaining !== savedContent);
    } catch (error) {
      store.setError(error);
    } finally {
      setSaving(false);
    }
  };
  return (
    <div className="panel-body">
      {!!thread.layout?.files?.length && (
        <div className="file-tabs">
          {thread.layout.files.map((tab) => (
            <div className="file-tab" key={tab.path}>
              <Button
                variant="ghost"
                className={`small-button ${file === tab.path ? "selected" : ""}`}
                onClick={() => {
                  try {
                    store.openFile(tab);
                  } catch (error) {
                    store.setError(error);
                  }
                }}
              >
                {tab.path.split("/").at(-1)}
                {thread.fileDrafts?.[tab.path] &&
                thread.fileDrafts[tab.path].content !==
                  thread.fileDrafts[tab.path].original
                  ? " •"
                  : ""}
              </Button>
              <Button
                variant="ghost"
                className="small-button"
                aria-label={`Close ${tab.path}`}
                disabled={saving}
                onClick={() => close(tab.path)}
              >
                ×
              </Button>
            </div>
          ))}
        </div>
      )}
      <div className="review-toolbar">
        <span>{path || "Project files"}</span>
        {path && (
          <Button
            variant="ghost"
            className="small-button"
            onClick={() => void load(path.split("/").slice(0, -1).join("/"))}
          >
            Up
          </Button>
        )}
      </div>
      {!file ? (
        <div className="file-browser">
          {entries.map((entry) => (
            <Button
              variant="ghost"
              key={entry.path}
              disabled={entry.kind === "symlink"}
              onClick={() => {
                if (entry.kind === "directory") void load(entry.path);
                else if (entry.kind === "file") {
                  try {
                    store.openFile({ path: entry.path });
                  } catch (error) {
                    store.setError(error);
                  }
                }
              }}
            >
              {entry.name}
              {entry.kind === "directory" ? "/" : ""}
            </Button>
          ))}
          {!entries.length && (
            <p className="muted">This directory has no visible files.</p>
          )}
        </div>
      ) : (
        <>
          <strong>{file}</strong>
          <Textarea
            ref={editor}
            className="file-editor"
            aria-label={`Edit ${file}`}
            value={content}
            onChange={(event) => {
              const current = currentThread(),
                next = event.target.value,
                original = current.fileDrafts?.[file]?.original ?? content;
              setContent(next);
              setDirty(next !== original);
              store.updateThread(thread.id, {
                fileDrafts: {
                  ...current.fileDrafts,
                  [file]: { content: next, original },
                },
              });
            }}
          />
          <div className="permission-actions">
            <Button
              variant="ghost"
              className="small-button"
              disabled={!dirty || saving}
              onClick={() => void save()}
            >
              {saving ? "Saving…" : "Save file"}
            </Button>
            <Button
              variant="ghost"
              className="small-button"
              disabled={saving}
              onClick={() => {
                if (
                  !dirty ||
                  window.confirm(
                    `Discard the unsaved draft for ${file} and reload from disk?`,
                  )
                )
                  void open(file, true);
              }}
            >
              Reload file
            </Button>
            <Button
              variant="ghost"
              className="small-button"
              onClick={() =>
                store.updateThread(thread.id, {
                  layout: { ...currentThread().layout, activeFile: undefined },
                })
              }
            >
              Back to files
            </Button>
            <Button
              variant="ghost"
              className="small-button"
              onClick={() => {
                const start = editor.current?.selectionStart ?? 0,
                  end = editor.current?.selectionEnd ?? 0,
                  selected = end > start;
                try {
                  store.addContext(thread.id, {
                    kind: "file",
                    path: file,
                    label: file,
                    text: selected ? content.slice(start, end) : content,
                    startLine: selected
                      ? content.slice(0, start).split("\n").length
                      : 1,
                    endLine: content
                      .slice(0, selected ? end : content.length)
                      .split("\n").length,
                  });
                } catch (error) {
                  store.setError(error);
                }
              }}
            >
              Add to message
            </Button>
          </div>
        </>
      )}
    </div>
  );
}
function TerminalView({ store, thread, extensionId, view }: Host) {
  const terminalRef = useRef<Terminal>(null);
  const host = useRef<HTMLDivElement>(null);
  const [terminalId, setTerminalId] = useState("");
  const [status, setStatus] = useState("Starting terminal…");
  useEffect(() => {
    let disposed = false;
    let id = "";
    let after = 0;
    let pending = false;
    const terminal = new Terminal({
      fontSize: 12,
      fontFamily: "monospace",
      theme: { background: "#101010", foreground: "#dedede" },
      cursorBlink: true,
    });
    terminalRef.current = terminal;
    const fit = new FitAddon();
    terminal.loadAddon(fit);
    terminal.open(host.current!);
    fit.fit();
    const writes = createTerminalInput(
      (data) => store.invokeExtension(extensionId, "terminal.write", {
        terminalId: id,
        data,
      }),
      (error) => {
        setStatus("Input interrupted · reopen the terminal to continue");
        store.setError(error);
      },
    );
    const input = terminal.onData((data) => {
      if (id && !disposed) writes.write(data);
    });
    const resize = new ResizeObserver(() => {
      fit.fit();
      if (id)
        void store
          .invokeExtension(extensionId, "terminal.resize", {
            terminalId: id,
            cols: terminal.cols,
            rows: terminal.rows,
          })
          .catch((e) => store.setError(e));
    });
    resize.observe(host.current!);
    void store
      .invokeExtension<{ terminalId: string }>(extensionId, view.command, {
        projectId: thread.worktreeId,
        cols: terminal.cols,
        rows: terminal.rows,
      })
      .then((result) => {
        id = result.terminalId;
        if (disposed) {
          void store.invokeExtension(extensionId, "terminal.close", {
            terminalId: id,
          });
          return;
        }
        setTerminalId(id);
        setStatus("Terminal running");
      })
      .catch((e) => {
        setStatus("Terminal could not start");
        store.setError(e);
      });
    const timer = setInterval(() => {
      if (!id || pending || disposed) return;
      pending = true;
      void store
        .invokeExtension<{
          text: string;
          next: number;
          exited: boolean;
          exitCode?: number;
        }>(extensionId, "terminal.read", { terminalId: id, after })
        .then((result) => {
          if (disposed) return;
          after = result.next;
          terminal.write(result.text);
          if (result.exited) setStatus(`Exited · ${result.exitCode ?? 0}`);
        })
        .catch((e) => store.setError(e))
        .finally(() => {
          pending = false;
        });
    }, 150);
    return () => {
      disposed = true;
      clearInterval(timer);
      resize.disconnect();
      input.dispose();
      writes.dispose();
      terminal.dispose();
      terminalRef.current = null;
      if (id)
        void store
          .invokeExtension(extensionId, "terminal.close", { terminalId: id })
          .catch(() => {});
    };
  }, [thread.worktreeId, extensionId, view.command]);
  return (
    <div className="terminal-panel">
      <p className="panel-note">
        {status} · {terminalId.slice(0, 8)}
      </p>
      <div className="terminal-host" ref={host} />
      <Button
        variant="ghost"
        className="small-button"
        onClick={() => {
          const terminal = terminalRef.current;
          const text = terminal?.getSelection();
          if (!text) {
            store.setError("Select terminal output to add it to your message.");
            return;
          }
          try {
            store.addContext(thread.id, {
              kind: "terminal",
              label: `Terminal · ${terminalId.slice(0, 8)}`,
              text,
              sourceId: terminalId,
            });
          } catch (e) {
            store.setError(e);
          }
        }}
      >
        Add selected output to message
      </Button>
    </div>
  );
}
function Actions({ store, thread, extensionId, view }: Host) {
  const [actions, setActions] = useState<
    { id: string; title: string; program: string; args: string[] }[]
  >([]);
  const [result, setResult] = useState("");
  const [running, setRunning] = useState("");
  const [starting, setStarting] = useState(false);
  const startingRef = useRef(false);
  const active = useRef("");
  const lifecycle = useRef(0);
  useEffect(() => {
    lifecycle.current++;
    return () => {
      lifecycle.current++;
    };
  }, [thread.worktreeId]);
  useEffect(() => {
    void store
      .invokeExtension<{ actions: typeof actions }>(extensionId, view.command, {
        projectId: thread.worktreeId,
      })
      .then((r) => setActions(r.actions))
      .catch((e) => store.setError(e));
  }, [thread.worktreeId]);
  useEffect(
    () => () => {
      if (active.current)
        void store
          .invokeExtension(extensionId, "terminal.close", {
            terminalId: active.current,
          })
          .catch(() => {});
    },
    [],
  );
  useEffect(() => {
    if (!running) return;
    let offset = 0;
    let pending = false;
    const timer = setInterval(() => {
      if (pending) return;
      pending = true;
      void store
        .invokeExtension<{
          text: string;
          next: number;
          exited: boolean;
          exitCode?: number;
        }>(extensionId, "terminal.read", { terminalId: running, after: offset })
        .then((r) => {
          offset = r.next;
          setResult(
            (p) => p + r.text.replace(/\x1b\[[0-9;?]*[ -/]*[@-~]/g, ""),
          );
          if (r.exited) {
            setResult((p) => `${p}\nExit code: ${r.exitCode ?? 0}`);
            active.current = "";
            setRunning("");
            void store
              .invokeExtension(extensionId, "terminal.close", {
                terminalId: running,
              })
              .catch(() => {});
          }
        })
        .catch((e) => store.setError(e))
        .finally(() => {
          pending = false;
        });
    }, 200);
    return () => clearInterval(timer);
  }, [running]);
  return (
    <div className="panel-body">
      <h3>Project actions</h3>
      {actions.map((action) => (
        <div className="project-action" key={action.id}>
          <strong>{action.title}</strong>
          <code>{[action.program, ...action.args].join(" ")}</code>
          <Button
            variant="ghost"
            className="small-button"
            disabled={!!running || starting}
            onClick={() => {
              if (startingRef.current || active.current) return;
              startingRef.current = true;
              setStarting(true);
              setResult("");
              const generation = lifecycle.current;
              void store
                .invokeExtension<{ terminalId: string }>(
                  extensionId,
                  "actions.run",
                  { projectId: thread.worktreeId, actionId: action.id },
                )
                .then((r) => {
                  if (generation !== lifecycle.current) {
                    void store
                      .invokeExtension(extensionId, "terminal.close", {
                        terminalId: r.terminalId,
                      })
                      .catch(() => {});
                    return;
                  }
                  active.current = r.terminalId;
                  setRunning(r.terminalId);
                })
                .catch((e) => store.setError(e))
                .finally(() => {
                  startingRef.current = false;
                  if (generation === lifecycle.current) setStarting(false);
                });
            }}
          >
            Run
          </Button>
        </div>
      ))}
      {!actions.length && (
        <p className="muted">
          No project actions found. Package scripts are discovered
          automatically.
        </p>
      )}
      {running && (
        <Button
          variant="ghost"
          className="small-button"
          onClick={() => {
            const id = running;
            active.current = "";
            setRunning("");
            void store
              .invokeExtension(extensionId, "terminal.close", {
                terminalId: id,
              })
              .catch((e) => store.setError(e));
          }}
        >
          Stop action
        </Button>
      )}
      {result && <pre className="action-output">{result}</pre>}
    </div>
  );
}
function GitReview({ store, thread, extensionId, view }: Host) {
  const [scope, setScope] = useState("working");
  const [base, setBase] = useState("HEAD");
  const [branches, setBranches] = useState<string[]>([]);
  const [review, setReview] = useState<Review>();
  const [file, setFile] = useState("");
  const [checkpoints, setCheckpoints] = useState<
    { turnId: string; base: string; end?: string | null }[]
  >([]);
  const [turnId, setTurnId] = useState("");
  const [boundary, setBoundary] = useState("before");
  const [restoring, setRestoring] = useState(false);
  const epoch = useRef(0);
  const [hashes, setHashes] = useState<Record<string, string>>({});
  useEffect(
    () => () => {
      epoch.current++;
    },
    [],
  );
  const project = store
    .getSnapshot()
    .projects.find((p) => p.id === thread.projectId);
  const args = {
    projectId: project?.id ?? thread.projectId,
    worktreeId: thread.worktreeId,
  };
  const load = async () => {
    const request = ++epoch.current;
    try {
      const result = await store.invokeExtension<Review>(
        extensionId,
        view.command,
        {
          ...args,
          scope,
          base,
          file: file || undefined,
          sessionId: thread.sessionId,
          turnId: turnId || undefined,
        },
      );
      if (request !== epoch.current) return;
      setReview(result);
      const pairs = await Promise.all(
        result.files.map(async (changed) => {
          const scoped = await store.invokeExtension<Review>(
            extensionId,
            view.command,
            {
              ...args,
              scope,
              base,
              file: changed.path,
              sessionId: thread.sessionId,
              turnId: turnId || undefined,
            },
          );
          return [changed.path, await fingerprint(scoped.diff)] as const;
        }),
      );
      if (request === epoch.current) setHashes(Object.fromEntries(pairs));
    } catch (error) {
      if (request === epoch.current) store.setError(error);
    }
  };

  useEffect(() => {
    void store
      .invokeExtension<{ branches: string[] }>(extensionId, "branches", args)
      .then((r) => setBranches(r.branches))
      .catch((e) => store.setError(e));
    if (thread.sessionId)
      void store
        .invokeExtension<{ checkpoints: typeof checkpoints }>(
          extensionId,
          "checkpoints",
          { sessionId: thread.sessionId },
        )
        .then((r) => setCheckpoints(r.checkpoints))
        .catch((e) => store.setError(e));
  }, [thread.projectId, thread.worktreeId, thread.sessionId, thread.state]);
  useEffect(() => {
    void load();
  }, [scope, base, file, turnId, thread.worktreeId]);
  const stage = async (path: string, command: string) => {
    try {
      await store.invokeExtension(extensionId, command, {
        ...args,
        paths: [path],
      });
      await load();
    } catch (e) {
      store.setError(e);
    }
  };
  return (
    <div className="panel-body">
      <label>
        Review scope
        <Select label="Review scope" value={scope} onChange={(value) => { setScope(value); setFile(""); }}
          options={["working", "staged", "unstaged", "base", "turn"].map((value) => ({ id: value, label: value }))} />
      </label>
      {scope === "base" && (
        <label>
          Base ref
          <ComboBox label="Review base" value={base} onChange={setBase} options={branches} />
        </label>
      )}
      {scope === "turn" && (
        <label>
          Turn
          <Select label="Review turn" value={turnId} onChange={setTurnId}
            options={[{ id: "", label: "Select a completed turn" }, ...checkpoints.filter((checkpoint) => checkpoint.end).map((checkpoint, index) => ({ id: checkpoint.turnId, label: `Turn ${index + 1} · ${checkpoint.turnId.slice(0, 8)}` }))]} />
        </label>
      )}
      {scope === "turn" &&
        turnId &&
        project?.worktrees.some(
          (tree) => tree.id === thread.worktreeId && !tree.isMain,
        ) && (
          <Disclosure>
            <Disclosure.Summary>Restore files from this checkpoint</Disclosure.Summary>
            <p className="panel-note">
              Requires a clean managed worktree and all conversations using it
              closed. Restores files and the index; the branch and conversation
              stay in place.
            </p>
            <label>
              Checkpoint boundary
              <Select label="Checkpoint boundary" value={boundary} onChange={setBoundary}
                options={[{ id: "before", label: "Before this turn" }, { id: "after", label: "After this turn" }]} />
            </label>
            <Button
              variant="ghost"
              className="small-button danger-button"
              disabled={restoring || thread.state !== "closed"}
              onClick={() => {
                if (
                  !window.confirm(
                    `Restore the managed worktree files to ${boundary} this turn?`,
                  )
                )
                  return;
                setRestoring(true);
                void store
                  .invokeExtension(extensionId, "checkpoint.restore", {
                    ...args,
                    sessionId: thread.sessionId,
                    turnId,
                    boundary,
                  })
                  .then(load)
                  .catch((error) => store.setError(error))
                  .finally(() => setRestoring(false));
              }}
            >
              Restore worktree files
            </Button>
          </Disclosure>
        )}
      <Button
        variant="ghost"
        className="small-button"
        onClick={() => void load()}
      >
        Refresh changes
      </Button>
      <div className="changed-files">
        {review?.files.map((f) => (
          <div key={f.path}>
            <div className="reviewed-checkbox">
              <Checkbox label={`Mark ${f.path} reviewed`} hideLabel
                disabled={!hashes[f.path]}
                checked={
                  !!hashes[f.path] &&
                  thread.reviewedFiles?.[
                    reviewKey(
                      thread.worktreeId,
                      scope,
                      scope === "turn" ? turnId : base,
                      f.path,
                    )
                  ] === hashes[f.path]
                }
                onChange={(checked) => {
                  const key = reviewKey(
                    thread.worktreeId,
                    scope,
                    scope === "turn" ? turnId : base,
                    f.path,
                  );
                  const marks = { ...thread.reviewedFiles };
                  if (checked) marks[key] = hashes[f.path];
                  else delete marks[key];
                  store.updateThread(thread.id, { reviewedFiles: marks });
                }}
              />
            </div>
            <Button
              variant="ghost"
              className="file-review-name"
              onClick={() => setFile(file === f.path ? "" : f.path)}
            >
              {f.status} {f.path}
            </Button>
            {["working", "unstaged"].includes(scope) && (
              <Button
                variant="ghost"
                className="small-button"
                onClick={() => void stage(f.path, "stage")}
              >
                Stage
              </Button>
            )}
            {["working", "staged"].includes(scope) && (
              <Button
                variant="ghost"
                className="small-button"
                onClick={() => void stage(f.path, "unstage")}
              >
                Unstage
              </Button>
            )}
          </div>
        ))}
      </div>
      <pre className="patch">{review?.diff || "No changes in this scope."}</pre>
      <Button
        variant="ghost"
        className="small-button"
        disabled={!review?.diff}
        onClick={() => {
          try {
            store.addContext(thread.id, {
              kind: "review",
              label: `${scope} diff${file ? ` · ${file}` : ""}`,
              text: window.getSelection()?.toString() || review!.diff,
              sourceId: `${thread.worktreeId}:${scope}:${turnId || base}`,
            });
          } catch (e) {
            store.setError(e);
          }
        }}
      >
        Add diff to message
      </Button>
      {review?.truncated && (
        <p className="panel-note">Review output is truncated.</p>
      )}
    </div>
  );
}
function CommandView({ store, thread, extensionId, view }: Host) {
  const [args, setArgs] = useState(
    JSON.stringify(
      { projectId: thread.worktreeId, sessionId: thread.sessionId },
      null,
      2,
    ),
  );
  const [result, setResult] = useState<unknown>();
  const [busy, setBusy] = useState(false);
  return (
    <div className="panel-body">
      <h3>{view.title}</h3>
      <label>
        Command arguments
        <Textarea
          aria-label="Extension command arguments"
          value={args}
          onChange={(e) => setArgs(e.target.value)}
        />
      </label>
      <Button
        variant="ghost"
        className="small-button"
        disabled={busy}
        onClick={() => {
          setBusy(true);
          try {
            const parsed = JSON.parse(args);
            void store
              .invokeExtension(extensionId, view.command, parsed)
              .then(setResult)
              .catch((e) => store.setError(e))
              .finally(() => setBusy(false));
          } catch (e) {
            store.setError(e);
            setBusy(false);
          }
        }}
      >
        {busy ? "Running…" : "Run command"}
      </Button>
      {result != null && (
        <pre className="action-output">{JSON.stringify(result, null, 2)}</pre>
      )}
    </div>
  );
}
export function ExtensionPanel({
  store,
  thread,
  extensions,
}: {
  store: WorkspaceStore;
  thread: Thread;
  extensions: ExtensionInfo[];
}) {
  const views = extensions.flatMap((extension) =>
    (extension.views ?? [])
      .filter(
        (view) =>
          !["git-review", "git-workflow"].includes(view.kind) ||
          store.getSnapshot().projects.find((p) => p.id === thread.projectId)
            ?.git,
      )
      .map((view) => ({
        view,
        extensionId: extension.id,
      })),
  );
  const [selected, setSelected] = useState(
    thread.layout?.selectedView ??
      (views[0] ? `${views[0].extensionId}:${views[0].view.id}` : ""),
  );
  const [opened, setOpened] = useState<string[]>(
    thread.layout?.openViews ?? (selected ? [selected] : []),
  );
  const [registered, setRegistered] = useState(false);
  useEffect(() => {
    const id = thread.layout?.selectedView;
    if (id !== undefined) {
      setSelected(id);
      if (id)
        setOpened((ids) => (ids.includes(id) ? ids : [...ids, id].slice(-8)));
    }
  }, [thread.layout?.selectedView]);
  useEffect(() => {
    let disposed = false;
    setRegistered(false);
    if (thread.worktreeId && extensions.some((e) => e.id === "workspace"))
      void store
        .openWorkspace(thread)
        .then(() => {
          if (!disposed) setRegistered(true);
        })
        .catch((error) => {
          if (!disposed) store.setError(error);
        });
    else setRegistered(true);
    return () => {
      disposed = true;
    };
  }, [thread.worktreeId, extensions.some((e) => e.id === "workspace")]);
  const choose = (id: string) => {
    if (!opened.includes(id) && opened.length >= 8) {
      store.setError(
        "Keep up to eight tool views open. Close another view first.",
      );
      return;
    }
    const next = opened.includes(id) ? opened : [...opened, id];
    setOpened(next);
    setSelected(id);
    store.updateThread(thread.id, {
      layout: { ...thread.layout, selectedView: id, openViews: next },
    });
  };
  const close = (id: string) => {
    const next = opened.filter((value) => value !== id),
      active = selected === id ? (next.at(-1) ?? "") : selected;
    setOpened(next);
    setSelected(active);
    store.updateThread(thread.id, {
      layout: { ...thread.layout, selectedView: active, openViews: next },
    });
  };
  const render = (current: (typeof views)[number]) => {
    const props = { store, thread };
    switch (current.view.kind) {
      case "git-workflow":
        return <GitWorkflow {...props} />;
      case "preview":
        return <BrowserPreview {...props} />;
      case "devices":
        return <Devices {...props} />;
      case "windows":
        return <SystemWindows {...props} />;
      case "web":
        return (
          <CustomExtensionView
            {...props}
            extension={extensions.find((e) => e.id === current.extensionId)!}
            view={current.view}
          />
        );
      case "schedules":
        return <Schedules {...props} />;
      case "peers":
        return <Peers {...props} />;
      case "usage":
        return <UsageDashboard {...props} />;
      case "resources":
        return <ResourceLibrary {...props} />;
      case "settings":
        return <ProjectSettings {...props} />;
      case "files":
        return <Files {...current} {...props} />;
      case "terminal":
        return <TerminalView {...current} {...props} />;
      case "actions":
      case "tests":
        return <Actions {...current} {...props} />;
      case "git-review":
        return <GitReview {...current} {...props} />;
      default:
        return <CommandView {...current} {...props} />;
    }
  };
  return (
    <>
      <Button
        variant="ghost"
        className="small-button"
        onClick={() =>
          void store.refreshExtensions().catch((e) => store.setError(e))
        }
      >
        Reload extensions
      </Button>
      <label className="extension-picker">
        Open tool
        <Select label="Open tool view" value={selected} onChange={(value) => { if (value) choose(value); }}
          options={[{ id: "", label: "Choose a tool…" }, ...views.map(({ view, extensionId }) => ({ id: `${extensionId}:${view.id}`, label: view.title }))]} />
      </label>
      {opened.length > 0 && (
        <div className="open-view-tabs">
          {opened.map((id) => {
            const entry = views.find(
              (value) => `${value.extensionId}:${value.view.id}` === id,
            );
            return (
              entry && (
                <div className="file-tab" key={id}>
                  <Button
                    variant="ghost"
                    className={`small-button ${selected === id ? "selected" : ""}`}
                    onClick={() => choose(id)}
                  >
                    {entry.view.title}
                  </Button>
                  <Button
                    variant="ghost"
                    className="small-button"
                    aria-label={`Close ${entry.view.title}`}
                    onClick={() => close(id)}
                  >
                    ×
                  </Button>
                </div>
              )
            );
          })}
        </div>
      )}
      {!thread.worktreeId ? (
        <p className="panel-note">Open a project to use workspace tools.</p>
      ) : !registered ? (
        <p className="panel-note">Opening workspace…</p>
      ) : (
        <>
          {views
            .filter((value) =>
              opened.includes(`${value.extensionId}:${value.view.id}`),
            )
            .map((entry) => {
              const id = `${entry.extensionId}:${entry.view.id}`;
              return (
                <div
                  className="extension-view"
                  key={id}
                  hidden={selected !== id}
                >
                  {render(entry)}
                </div>
              );
            })}
          {!opened.length && (
            <p className="panel-note">Choose a tool view to open it.</p>
          )}
          {!views.length && (
            <p className="panel-note">No extension views are available.</p>
          )}
        </>
      )}
    </>
  );
}
