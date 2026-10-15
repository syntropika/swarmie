import { FitAddon } from "@xterm/addon-fit";
import { Terminal } from "@xterm/xterm";
import { useEffect, useRef, useState, useCallback, useMemo, useId } from "react";

import { useConfirmation } from "~/src/components/confirmation";
import { useExtensionQuery } from "~/src/components/extension-query";
import { extensionResult } from "~/src/components/extension-result";
import { hasCount, hasText } from "~/src/components/presentation-values";
import { Button } from "~/src/components/ui/button";
import { Checkbox } from "~/src/components/ui/checkbox";
import { ComboBox } from "~/src/components/ui/combobox";

import "@xterm/xterm/css/xterm.css";
import { Disclosure } from "~/src/components/ui/disclosure";
import { Select } from "~/src/components/ui/select";
import { Textarea } from "~/src/components/ui/textarea";
import { decodeReviewResult } from "~/src/connections";
import { CustomExtensionView } from "~/src/custom-extension-view";
import { GitWorkflow } from "~/src/git-workflow";
import { BrowserPreview, Devices, SystemWindows } from "~/src/platform-views";
import { ProjectSettings } from "~/src/project-settings";
import type { ExtensionInfo, ExtensionView, Thread, Review } from "~/src/protocol";
import { ResourceLibrary } from "~/src/resource-library";
import { fingerprint, reviewKey } from "~/src/reviewed";
import { createTerminalInput } from "~/src/terminal-input";
import { Schedules, Peers, UsageDashboard } from "~/src/workflow-views";
import type { WorkspaceStore } from "~/src/workspace";

function selectedText(selection: string | undefined, fallback: string | undefined) {
  return hasText(selection) ? selection : (fallback ?? "");
}
type Host = {
  store: WorkspaceStore;
  thread: Thread;
  extensionId: string;
  view: ExtensionView;
};
function editorDraft(thread: Thread) {
  const activePath = thread.layout?.activeFile?.path ?? "";
  const draft = thread.fileDrafts?.[activePath];
  return {
    file: draft === undefined ? "" : activePath,
    content: draft?.content ?? "",
    dirty: draft !== undefined && draft.content !== draft.original,
  };
}
function Files({ store, thread, extensionId, view }: Host) {
  const confirmation = useConfirmation(thread.id);
  const editor = useRef<HTMLTextAreaElement>(null);
  const [path, setPath] = useState("");
  const { file, content, dirty } = editorDraft(thread);
  const [saving, setSaving] = useState(false);
  const requests = useRef({ list: 0, file: 0 });
  const currentThread = useCallback(() => {
    const current = store.getSnapshot().threads.find((t) => t.id === thread.id);
    if (current === undefined) {
      throw new Error("The conversation is no longer available.");
    }
    return current;
  }, [store, thread.id]);
  const args = useMemo(
    () => ({
      projectId: thread.worktreeId,
      ...(hasText(thread.sessionId) ? { sessionId: thread.sessionId } : {}),
    }),
    [thread.worktreeId, thread.sessionId],
  );
  const fetchEntries = async () => {
    const result = await store.invokeExtension<{
      entries: { name: string; path: string; kind: string }[];
    }>(
      extensionId,
      view.command,
      { ...args, path },
      extensionResult<{ entries: { name: string; path: string; kind: string }[] }>({
        type: "object",
        properties: {
          entries: {
            type: "array",
            items: {
              type: "object",
              properties: {
                name: { type: "string" },
                path: { type: "string" },
                kind: { type: "string" },
              },
              required: ["name", "path", "kind"],
            },
          },
        },
        required: ["entries"],
      }),
    );
    return result.entries;
  };
  const listing = useExtensionQuery(
    store,
    ["files", thread.id, args, extensionId, view.command, path],
    fetchEntries,
  );
  const entries = listing.data ?? [];
  const load = async (next: string) => {
    if (next === path) {
      await listing.refresh();
    } else {
      setPath(next);
    }
  };
  const open = useCallback(
    async (next: string, reload = false) => {
      requests.current.file += 1;
      const request = requests.current.file;
      try {
        await store.ready();
        if (request !== requests.current.file) {
          return;
        }
        const draft = currentThread().fileDrafts?.[next];
        if (draft && !reload) {
          return;
        }
        const result = await store.invokeExtension<{ content: string }>(
          extensionId,
          "files.read",
          {
            ...args,
            path: next,
          },
          extensionResult<{ content: string }>({
            type: "object",
            properties: { content: { type: "string" } },
            required: ["content"],
          }),
        );
        if (request !== requests.current.file) {
          return;
        }
        store.updateThread(thread.id, {
          fileDrafts: {
            ...currentThread().fileDrafts,
            [next]: { content: result.content, original: result.content },
          },
        });
      } catch (error) {
        store.setError(error);
      }
    },
    [store, args, thread.id, extensionId, currentThread],
  );
  useEffect(() => {
    const tracked = requests.current;
    return () => {
      tracked.file += 1;
    };
  }, []);
  useEffect(() => {
    const location = thread.layout?.activeFile;
    if (location) {
      open(location.path).catch(store.setError.bind(store));
    } else {
      requests.current.file += 1;
    }
  }, [thread.layout?.activeFile, store, open]);
  useEffect(() => {
    const location = thread.layout?.activeFile;
    if (!editor.current || file !== location?.path || !hasCount(location.startLine)) {
      return;
    }
    const lines = content.split("\n");
    const start =
      lines.slice(0, location.startLine - 1).join("\n").length + (location.startLine > 1 ? 1 : 0);
    const end = lines.slice(0, location.endLine ?? location.startLine).join("\n").length;
    editor.current.focus();
    editor.current.setSelectionRange(start, end);
    editor.current.scrollTop = Math.max(0, (location.startLine - 3) * 20);
  }, [file, content, thread.layout?.activeFile]);
  const discard = (closed: string) => {
    const current = currentThread();
    const files = current.layout?.files?.filter((tab) => tab.path !== closed) ?? [];
    const drafts = Object.fromEntries(
      Object.entries(current.fileDrafts ?? {}).filter(([entryKey]) => entryKey !== closed),
    );
    store.updateThread(thread.id, {
      fileDrafts: drafts,
      layout: {
        ...current.layout,
        files,
        activeFile:
          current.layout?.activeFile?.path === closed ? files.at(-1) : current.layout?.activeFile,
      },
    });
  };
  const close = (closed: string) => {
    const draft = currentThread().fileDrafts?.[closed];
    if (draft && draft.content !== draft.original) {
      confirmation.confirm(`Discard the unsaved draft for ${closed}?`, () => {
        discard(closed);
      });
    } else {
      discard(closed);
    }
  };
  const reloadFile = () => {
    open(file, true).catch(store.setError.bind(store));
  };
  const save = async () => {
    setSaving(true);
    const savedPath = file;
    const savedContent = content;
    try {
      const baseline = currentThread().fileDrafts?.[savedPath]?.original;
      await store.rpc("client/fileSave", {
        extensionId,
        baseline,
        arguments: { ...args, path: savedPath, content: savedContent },
      });
      const current = currentThread();
      const remaining = current.fileDrafts?.[savedPath]?.content ?? savedContent;
      store.updateThread(thread.id, {
        fileDrafts: {
          ...current.fileDrafts,
          [savedPath]: { content: remaining, original: savedContent },
        },
      });
    } catch (error) {
      store.setError(error);
    } finally {
      setSaving(false);
    }
  };
  return (
    <div className="panel-body">
      {confirmation.dialog}
      {hasCount(thread.layout?.files?.length) && (
        <div className="file-tabs">
          {thread.layout.files.map((tab) => (
            <div className="file-tab" key={tab.path}>
              <Button
                variant="ghost"
                size="sm"
                selected={file === tab.path}
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
                thread.fileDrafts[tab.path].content !== thread.fileDrafts[tab.path].original
                  ? " •"
                  : ""}
              </Button>
              <Button
                size="sm"
                variant="ghost"

                aria-label={`Close ${tab.path}`}
                disabled={saving}
                onClick={() => {
                  close(tab.path);
                }}
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
            size="sm"
            variant="ghost"

            onClick={() => {
              load(path.split("/").slice(0, -1).join("/")).catch(store.setError.bind(store));
            }}
          >
            Up
          </Button>
        )}
      </div>
      {file ? (
        <>
          <strong>{file}</strong>
          <Textarea
            ref={editor}
            variant="editor"
            aria-label={`Edit ${file}`}
            value={content}
            onChange={(event) => {
              const current = currentThread();
              const next = event.target.value;
              const original = current.fileDrafts?.[file]?.original ?? content;
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
              size="sm"
              variant="ghost"

              disabled={!dirty || saving}
              onClick={() => {
                save().catch(store.setError.bind(store));
              }}
            >
              {saving ? "Saving…" : "Save file"}
            </Button>
            <Button
              size="sm"
              variant="ghost"

              disabled={saving}
              onClick={() => {
                if (dirty) {
                  confirmation.confirm(
                    `Discard the unsaved draft for ${file} and reload from disk?`,
                    reloadFile,
                  );
                } else {
                  reloadFile();
                }
              }}
            >
              Reload file
            </Button>
            <Button
              size="sm"
              variant="ghost"

              onClick={() => {
                store.updateThread(thread.id, {
                  layout: { ...currentThread().layout, activeFile: undefined },
                });
              }}
            >
              Back to files
            </Button>
            <Button
              size="sm"
              variant="ghost"

              onClick={() => {
                const start = editor.current?.selectionStart ?? 0;
                const end = editor.current?.selectionEnd ?? 0;
                const selected = end > start;
                try {
                  store.addContext(thread.id, {
                    kind: "file",
                    path: file,
                    label: file,
                    text: selected ? content.slice(start, end) : content,
                    startLine: selected ? content.slice(0, start).split("\n").length : 1,
                    endLine: content.slice(0, selected ? end : content.length).split("\n").length,
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
      ) : (
        <div className="file-browser">
          {entries.map((entry) => (
            <Button
              variant="ghost"
              key={entry.path}
              disabled={entry.kind === "symlink"}
              onClick={() => {
                if (entry.kind === "directory") {
                  load(entry.path).catch(store.setError.bind(store));
                } else if (entry.kind === "file") {
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
          {!entries.length && <p className="muted">This directory has no visible files.</p>}
        </div>
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
    const handlePromiseFailure8 = (error: unknown) => {
      if (!disposed) {
        setStatus("Terminal could not start");
      }
      store.setError(error);
    };

    let id = "";
    let after = 0;
    let pending = false;
    const container = host.current;
    if (container === null) {
      return undefined;
    }
    const terminal = new Terminal({
      fontSize: 12,
      fontFamily: "monospace",
      theme: { background: "#101010", foreground: "#dedede" },
      cursorBlink: true,
    });
    terminalRef.current = terminal;
    const fit = new FitAddon();
    terminal.loadAddon(fit);
    terminal.open(container);
    fit.fit();
    const reportInputFailure = (failure: unknown) => {
      if (!disposed) {
        setStatus("Input interrupted · reopen the terminal to continue");
      }
      store.setError(failure);
    };
    const writes = createTerminalInput(
      async (data) =>
        await store.invokeExtension(extensionId, "terminal.write", {
          terminalId: id,
          ...(hasText(thread.sessionId) ? { sessionId: thread.sessionId } : {}),
          data,
        }),
      reportInputFailure,
    );
    const input = terminal.onData((data) => {
      if (id && !disposed) {
        writes.write(data);
      }
    });
    const resize = new ResizeObserver(() => {
      fit.fit();
      if (id) {
        const handlePromiseFailure7 = (e: unknown) => {
          store.setError(e);
        };

        store
          .invokeExtension(extensionId, "terminal.resize", {
            terminalId: id,
            cols: terminal.cols,
            rows: terminal.rows,
          })
          .catch(handlePromiseFailure7);
      }
    });
    resize.observe(container);
    const startTerminal = async () => {
      const result = await store.invokeExtension<{ terminalId: string }>(
        extensionId,
        view.command,
        {
          projectId: thread.worktreeId,
          ...(hasText(thread.sessionId) ? { sessionId: thread.sessionId } : {}),
          cols: terminal.cols,
          rows: terminal.rows,
        },
        extensionResult<{ terminalId: string }>({
          type: "object",
          properties: { terminalId: { type: "string" } },
          required: ["terminalId"],
        }),
      );
      id = result.terminalId;
      if (disposed) {
        await store.invokeExtension(extensionId, "terminal.close", { terminalId: id });
        return;
      }
      setTerminalId(id);
      setStatus("Terminal running");
    };
    startTerminal().catch(handlePromiseFailure8);
    const timer = setInterval(() => {
      if (!id || pending || disposed) {
        return;
      }
      pending = true;
      store
        .invokeExtension<{
          text: string;
          next: number;
          exited: boolean;
          exitCode?: number;
        }>(
          extensionId,
          "terminal.read",
          { terminalId: id, after },
          extensionResult<{
            text: string;
            next: number;
            exited: boolean;
            exitCode?: number;
          }>({
            type: "object",
            properties: {
              text: { type: "string" },
              next: { type: "number" },
              exited: { type: "boolean" },
              exitCode: { type: "number" },
            },
            required: ["text", "next", "exited"],
          }),
        )
        .then((result) => {
          if (disposed) {
            return;
          }
          after = result.next;
          terminal.write(result.text);
          if (result.exited) {
            setStatus(`Exited · ${result.exitCode ?? 0}`);
          }
        })
        .catch(store.setError.bind(store))
        .finally(() => {
          pending = false;
        })
        .catch(store.setError.bind(store));
    }, 150);
    return () => {
      disposed = true;
      clearInterval(timer);
      resize.disconnect();
      input.dispose();
      writes.dispose();
      terminal.dispose();
      terminalRef.current = null;
      if (id) {
        const handlePromiseFailure10 = store.setError.bind(store);

        store
          .invokeExtension(extensionId, "terminal.close", { terminalId: id })
          .catch(handlePromiseFailure10);
      }
    };
  }, [thread.worktreeId, extensionId, view.command, store, thread.sessionId]);
  return (
    <div className="terminal-panel">
      <p className="panel-note">
        {status} · {terminalId.slice(0, 8)}
      </p>
      <div className="terminal-host" ref={host} />
      <Button
        size="sm"
        variant="ghost"

        onClick={() => {
          const terminal = terminalRef.current;
          const text = terminal?.getSelection();
          if (!hasText(text)) {
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
          } catch (error) {
            store.setError(error);
          }
        }}
      >
        Add selected output to message
      </Button>
    </div>
  );
}
function plainTerminalOutput(text: string) {
  let output = "";
  for (let index = 0; index < text.length; index += 1) {
    if (text.codePointAt(index) === 27 && text[index + 1] === "[") {
      let end = index + 2;
      while (
        end < text.length &&
        (text.codePointAt(end) ?? 0) >= 32 &&
        (text.codePointAt(end) ?? 0) <= 63
      ) {
        end += 1;
      }
      if (
        end < text.length &&
        (text.codePointAt(end) ?? 0) >= 64 &&
        (text.codePointAt(end) ?? 0) <= 126
      ) {
        index = end;
        continue;
      }
    }
    output += text[index];
  }
  return output;
}
function terminalAccess(store: WorkspaceStore, extensionId: string) {
  return {
    read: async (terminalId: string, after: number) =>
      await store.invokeExtension<{
        text: string;
        next: number;
        exited: boolean;
        exitCode?: number;
      }>(
        extensionId,
        "terminal.read",
        { terminalId, after },
        extensionResult<{
          text: string;
          next: number;
          exited: boolean;
          exitCode?: number;
        }>({
          type: "object",
          properties: {
            text: { type: "string" },
            next: { type: "number" },
            exited: { type: "boolean" },
            exitCode: { type: "number" },
          },
          required: ["text", "next", "exited"],
        }),
      ),
    close: async (terminalId: string) =>
      await store.invokeExtension(extensionId, "terminal.close", { terminalId }),
  };
}
function Actions({ store, thread, extensionId, view }: Host) {
  const nativeTerminal = useMemo(() => terminalAccess(store, extensionId), [store, extensionId]);
  const [actions, setActions] = useState<
    { id: string; title: string; program: string; args: string[] }[]
  >([]);
  const [output, setOutput] = useState("");
  const [runningRun, setRunningRun] = useState<{
    id: string;
    access: ReturnType<typeof terminalAccess>;
  }>();
  const running = runningRun?.id ?? "";
  const [starting, setStarting] = useState(false);
  const startingRef = useRef(false);
  const active = useRef("");
  const lifecycle = useRef(0);
  useEffect(() => {
    lifecycle.current += 1;
    return () => {
      lifecycle.current += 1;
    };
  }, []);
  useEffect(() => {
    store
      .invokeExtension<{ actions: typeof actions }>(
        extensionId,
        view.command,
        {
          projectId: thread.worktreeId,
          ...(hasText(thread.sessionId) ? { sessionId: thread.sessionId } : {}),
        },
        extensionResult<{ actions: typeof actions }>({
          type: "object",
          properties: {
            actions: {
              type: "array",
              items: {
                type: "object",
                properties: {
                  id: { type: "string" },
                  title: { type: "string" },
                  program: { type: "string" },
                  args: { type: "array", items: { type: "string" } },
                },
                required: ["id", "title", "program", "args"],
              },
            },
          },
          required: ["actions"],
        }),
      )
      .then((r) => {
        setActions(r.actions);
      })
      .catch(store.setError.bind(store));
  }, [thread.worktreeId, extensionId, view.command, store, thread.sessionId]);
  useEffect(
    () => () => {
      if (active.current) {
        const handlePromiseFailure12 = store.setError.bind(store);

        store
          .invokeExtension(extensionId, "terminal.close", {
            terminalId: active.current,
          })
          .catch(handlePromiseFailure12);
      }
    },
    [extensionId, store],
  );
  useEffect(() => {
    if (runningRun === undefined) {
      return undefined;
    }
    const { id: ownedTerminalId, access } = runningRun;
    let offset = 0;
    let pending = false;
    let disposed = false;
    const poll = async () => {
      if (pending || disposed) {
        return;
      }
      pending = true;
      try {
        const result = await access.read(ownedTerminalId, offset);
        if (disposed) {
          return;
        }
        offset = result.next;
        setOutput((previous) => previous + plainTerminalOutput(result.text));
        if (result.exited) {
          setOutput((previous) => `${previous}\nExit code: ${result.exitCode ?? 0}`);
          active.current = "";
          setRunningRun(undefined);
          await access.close(ownedTerminalId);
        }
      } finally {
        pending = false;
      }
    };
    const timer = setInterval(() => {
      poll().catch(store.setError.bind(store));
    }, 200);
    return () => {
      disposed = true;
      clearInterval(timer);
    };
  }, [runningRun, store]);
  return (
    <div className="panel-body">
      <h3>Project actions</h3>
      {actions.map((action) => (
        <div className="project-action" key={action.id}>
          <strong>{action.title}</strong>
          <code>{[action.program, ...action.args].join(" ")}</code>
          <Button
            size="sm"
            variant="ghost"

            disabled={!!running || starting}
            onClick={() => {
              if (startingRef.current || active.current) {
                return;
              }
              startingRef.current = true;
              setStarting(true);
              setOutput("");
              const generation = lifecycle.current;
              const startAction = async () => {
                try {
                  const result = await store.invokeExtension<{ terminalId: string }>(
                    extensionId,
                    "actions.run",
                    {
                      projectId: thread.worktreeId,
                      ...(hasText(thread.sessionId) ? { sessionId: thread.sessionId } : {}),
                      actionId: action.id,
                    },
                    extensionResult<{ terminalId: string }>({
                      type: "object",
                      properties: { terminalId: { type: "string" } },
                      required: ["terminalId"],
                    }),
                  );
                  if (generation !== lifecycle.current) {
                    await store.invokeExtension(extensionId, "terminal.close", {
                      terminalId: result.terminalId,
                    });
                    return;
                  }
                  active.current = result.terminalId;
                  setRunningRun({ id: result.terminalId, access: nativeTerminal });
                } finally {
                  startingRef.current = false;
                  if (generation === lifecycle.current) {
                    setStarting(false);
                  }
                }
              };
              startAction().catch(store.setError.bind(store));
            }}
          >
            Run
          </Button>
        </div>
      ))}
      {!actions.length && (
        <p className="muted">
          No project actions found. Package scripts are discovered automatically.
        </p>
      )}
      {running && (
        <Button
          size="sm"
          variant="ghost"

          onClick={() => {
            const id = running;
            active.current = "";
            setRunningRun(undefined);
            store
              .invokeExtension(extensionId, "terminal.close", {
                terminalId: id,
              })
              .catch(store.setError.bind(store));
          }}
        >
          Stop action
        </Button>
      )}
      {output && <pre className="action-output">{output}</pre>}
    </div>
  );
}
function GitReview({ store, thread, extensionId, view }: Host) {
  const confirmation = useConfirmation(thread.id);
  const fieldPrefix = useId();

  const [scope, setScope] = useState("working");
  const [base, setBase] = useState("HEAD");
  const [branches, setBranches] = useState<string[]>([]);

  const [file, setFile] = useState("");
  const [checkpoints, setCheckpoints] = useState<
    { turnId: string; base: string; end?: string | null }[]
  >([]);
  const [turnId, setTurnId] = useState("");
  const [boundary, setBoundary] = useState("before");
  const [restoring, setRestoring] = useState(false);
  const epoch = useRef(0);

  useEffect(
    () => () => {
      epoch.current += 1;
    },
    [],
  );
  const project = store.getSnapshot().projects.find((p) => p.id === thread.projectId);
  const args = useMemo(
    () => ({
      projectId: project?.id ?? thread.projectId,
      ...(hasText(thread.sessionId) ? { sessionId: thread.sessionId } : {}),
      worktreeId: thread.worktreeId,
    }),
    [project?.id, thread.projectId, thread.sessionId, thread.worktreeId],
  );
  const fetchReview = useCallback(async () => {
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
      decodeReviewResult,
    );

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
          decodeReviewResult,
        );
        return [changed.path, await fingerprint(scoped.diff)] as const;
      }),
    );
    return { review: result, hashes: Object.fromEntries(pairs) };
  }, [file, base, store, thread.sessionId, extensionId, args, view.command, scope, turnId]);

  const projection = useExtensionQuery(
    store,
    ["review", thread.id, args, extensionId, view.command, scope, base, file, turnId],
    fetchReview,
  );
  const review = projection.data?.review;
  const hashes = projection.data?.hashes ?? {};
  const load = projection.refresh;

  useEffect(() => {
    store
      .invokeExtension<{ branches: string[] }>(
        extensionId,
        "branches",
        args,
        extensionResult<{ branches: string[] }>({
          type: "object",
          properties: { branches: { type: "array", items: { type: "string" } } },
          required: ["branches"],
        }),
      )
      .then((r) => {
        setBranches(r.branches);
      })
      .catch(store.setError.bind(store));
    if (hasText(thread.sessionId)) {
      const handlePromiseFailure17 = (e: unknown) => {
        store.setError(e);
      };

      store
        .invokeExtension<{ checkpoints: typeof checkpoints }>(
          extensionId,
          "checkpoints",
          {
            sessionId: thread.sessionId,
          },
          extensionResult<{ checkpoints: typeof checkpoints }>({
            type: "object",
            properties: {
              checkpoints: {
                type: "array",
                items: {
                  type: "object",
                  properties: {
                    turnId: { type: "string" },
                    base: { type: "string" },
                    end: { anyOf: [{ type: "string" }, { type: "null" }] },
                  },
                  required: ["turnId", "base"],
                },
              },
            },
            required: ["checkpoints"],
          }),
        )
        .then((r) => {
          setCheckpoints(r.checkpoints);
        })
        .catch(handlePromiseFailure17);
    }
  }, [thread.sessionId, extensionId, args, store]);

  const stage = async (path: string, command: string) => {
    try {
      await store.invokeExtension(extensionId, command, {
        ...args,
        paths: [path],
      });
      await load();
    } catch (error) {
      store.setError(error);
    }
  };
  return (
    <div className="panel-body">
      {confirmation.dialog}
      <label htmlFor={`${fieldPrefix}-field-1`}>
        Review scope
        <Select
          id={`${fieldPrefix}-field-1`}
          label="Review scope"
          value={scope}
          onChange={(value) => {
            setScope(value);
            setFile("");
          }}
          options={["working", "staged", "unstaged", "base", "turn"].map((value) => ({
            id: value,
            label: value,
          }))}
        />
      </label>
      {scope === "base" && (
        <label htmlFor={`${fieldPrefix}-field-2`}>
          Base ref
          <ComboBox
            id={`${fieldPrefix}-field-2`}
            label="Review base"
            value={base}
            onChange={setBase}
            options={branches}
          />
        </label>
      )}
      {scope === "turn" && (
        <label htmlFor={`${fieldPrefix}-field-3`}>
          Turn
          <Select
            id={`${fieldPrefix}-field-3`}
            label="Review turn"
            value={turnId}
            onChange={setTurnId}
            options={[
              { id: "", label: "Select a completed turn" },
              ...checkpoints
                .filter((checkpoint) => checkpoint.end !== undefined && checkpoint.end !== "")
                .map((checkpoint, index) => ({
                  id: checkpoint.turnId,
                  label: `Turn ${index + 1} · ${checkpoint.turnId.slice(0, 8)}`,
                })),
            ]}
          />
        </label>
      )}
      {scope === "turn" &&
        turnId &&
        project?.worktrees.some((tree) => tree.id === thread.worktreeId && !tree.isMain) ===
          true && (
          <Disclosure>
            <Disclosure.Summary>Restore files from this checkpoint</Disclosure.Summary>
            <p className="panel-note">
              Requires a clean managed worktree and all conversations using it closed. Restores
              files and the index; the branch and conversation stay in place.
            </p>
            <label htmlFor={`${fieldPrefix}-field-4`}>
              Checkpoint boundary
              <Select
                id={`${fieldPrefix}-field-4`}
                label="Checkpoint boundary"
                value={boundary}
                onChange={setBoundary}
                options={[
                  { id: "before", label: "Before this turn" },
                  { id: "after", label: "After this turn" },
                ]}
              />
            </label>
            <Button
              size="sm"
              variant="destructive"

              disabled={restoring || thread.state !== "closed"}
              onClick={() => {
                const restoreConfirmed = async () => {
                  setRestoring(true);
                  try {
                    await store.invokeExtension(extensionId, "checkpoint.restore", {
                      ...args,
                      sessionId: thread.sessionId,
                      turnId,
                      boundary,
                    });
                    await load();
                  } finally {
                    setRestoring(false);
                  }
                };
                confirmation.confirm(
                  `Restore the managed worktree files to ${boundary} this turn?`,
                  () => {
                    restoreConfirmed().catch(store.setError.bind(store));
                  },
                );
              }}
            >
              Restore worktree files
            </Button>
          </Disclosure>
        )}
      <Button
        size="sm"
        variant="ghost"
        onClick={() => {
          load().catch(store.setError.bind(store));
        }}
      >
        Refresh changes
      </Button>
      <div className="changed-files">
        {review?.files.map((f) => (
          <div key={f.path}>
            <div className="reviewed-checkbox">
              <Checkbox
                label={`Mark ${f.path} reviewed`}
                hideLabel
                disabled={!hashes[f.path]}
                checked={
                  !!hashes[f.path] &&
                  thread.reviewedFiles?.[
                    reviewKey(thread.worktreeId, scope, scope === "turn" ? turnId : base, f.path)
                  ] === hashes[f.path]
                }
                onChange={(checked) => {
                  const key = reviewKey(
                    thread.worktreeId,
                    scope,
                    scope === "turn" ? turnId : base,
                    f.path,
                  );
                  let marks = { ...thread.reviewedFiles };
                  if (checked) {
                    marks[key] = hashes[f.path];
                  } else {
                    marks = Object.fromEntries(
                      Object.entries(marks).filter(([entryKey]) => entryKey !== key),
                    );
                  }
                  store.updateThread(thread.id, { reviewedFiles: marks });
                }}
              />
            </div>
            <Button
              variant="file"

              onClick={() => {
                setFile(file === f.path ? "" : f.path);
              }}
            >
              {f.status} {f.path}
            </Button>
            {["working", "unstaged"].includes(scope) && (
              <Button
                size="sm"
                variant="ghost"

                onClick={() => {
                  stage(f.path, "stage").catch(store.setError.bind(store));
                }}
              >
                Stage
              </Button>
            )}
            {["working", "staged"].includes(scope) && (
              <Button
                size="sm"
                variant="ghost"

                onClick={() => {
                  stage(f.path, "unstage").catch(store.setError.bind(store));
                }}
              >
                Unstage
              </Button>
            )}
          </div>
        ))}
      </div>
      <pre className="patch">
        {review?.diff === undefined || review.diff === ""
          ? "No changes in this scope."
          : review.diff}
      </pre>
      <Button
        size="sm"
        variant="ghost"

        disabled={!hasText(review?.diff)}
        onClick={() => {
          try {
            store.addContext(thread.id, {
              kind: "review",
              label: `${scope} diff${file ? ` · ${file}` : ""}`,
              text: selectedText(window.getSelection()?.toString(), review?.diff),
              sourceId: `${thread.worktreeId}:${scope}:${turnId || base}`,
            });
          } catch (error) {
            store.setError(error);
          }
        }}
      >
        Add diff to message
      </Button>
      {review?.truncated === true && <p className="panel-note">Review output is truncated.</p>}
    </div>
  );
}
function CommandView({ store, thread, extensionId, view }: Host) {
  const fieldPrefix = useId();

  const [args, setArgs] = useState(
    JSON.stringify(
      {
        projectId: thread.worktreeId,
        ...(hasText(thread.sessionId) ? { sessionId: thread.sessionId } : {}),
        sessionId: thread.sessionId,
      },
      null,
      2,
    ),
  );
  const [result, setResult] = useState<unknown>();
  const [busy, setBusy] = useState(false);
  return (
    <div className="panel-body">
      <h3>{view.title}</h3>
      <label htmlFor={`${fieldPrefix}-field-5`}>
        Command arguments
        <Textarea
          id={`${fieldPrefix}-field-5`}
          aria-label="Extension command arguments"
          value={args}
          onChange={(e) => {
            setArgs(e.target.value);
          }}
        />
      </label>
      <Button
        size="sm"
        variant="ghost"

        disabled={busy}
        onClick={() => {
          setBusy(true);
          try {
            const parsed: unknown = JSON.parse(args);
            store
              .invokeExtension(extensionId, view.command, parsed)
              .then(setResult)
              .catch(store.setError.bind(store))
              .finally(() => {
                setBusy(false);
              })
              .catch(store.setError.bind(store));
          } catch (error) {
            store.setError(error);
            setBusy(false);
          }
        }}
      >
        {busy ? "Running…" : "Run command"}
      </Button>
      {result !== undefined && (
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
  const fieldPrefix = useId();

  const views = extensions.flatMap((extension) =>
    (extension.views ?? [])
      .filter(
        (view) =>
          !["git-review", "git-workflow"].includes(view.kind) ||
          store.getSnapshot().projects.find((p) => p.id === thread.projectId)?.git === true,
      )
      .map((view) => ({
        view,
        extensionId: extension.id,
      })),
  );
  const firstView = views.at(0);
  const selected =
    thread.layout?.selectedView ??
    (firstView === undefined ? "" : `${firstView.extensionId}:${firstView.view.id}`);
  const storedViews = thread.layout?.openViews ?? [];
  const opened =
    selected !== "" && !storedViews.includes(selected)
      ? [...storedViews, selected].slice(-8)
      : storedViews;
  const choose = (id: string) => {
    if (!opened.includes(id) && opened.length >= 8) {
      store.setError("Keep up to eight tool views open. Close another view first.");
      return;
    }
    const next = opened.includes(id) ? opened : [...opened, id];
    store.updateThread(thread.id, {
      layout: { ...thread.layout, selectedView: id, openViews: next },
    });
  };
  const close = (id: string) => {
    const next = opened.filter((value) => value !== id);
    const active = selected === id ? (next.at(-1) ?? "") : selected;
    store.updateThread(thread.id, {
      layout: { ...thread.layout, selectedView: active, openViews: next },
    });
  };
  const render = (current: (typeof views)[number]) => {
    const props = { store, thread };
    switch (current.view.kind) {
      case "git-workflow": {
        return <GitWorkflow {...props} />;
      }
      case "preview": {
        return <BrowserPreview {...props} />;
      }
      case "devices": {
        return <Devices {...props} />;
      }
      case "windows": {
        return <SystemWindows {...props} />;
      }
      case "web": {
        const extension = extensions.find((entry) => entry.id === current.extensionId);
        if (extension === undefined) {
          return null;
        }
        return <CustomExtensionView {...props} extension={extension} view={current.view} />;
      }
      case "schedules": {
        return <Schedules {...props} />;
      }
      case "peers": {
        return <Peers {...props} />;
      }
      case "usage": {
        return <UsageDashboard {...props} />;
      }
      case "resources": {
        return <ResourceLibrary {...props} />;
      }
      case "settings": {
        return <ProjectSettings {...props} />;
      }
      case "files": {
        return <Files {...current} {...props} />;
      }
      case "terminal": {
        return <TerminalView {...current} {...props} />;
      }
      case "actions":
      case "tests": {
        return <Actions {...current} {...props} />;
      }
      case "git-review": {
        return <GitReview {...current} {...props} />;
      }
      default: {
        return <CommandView {...current} {...props} />;
      }
    }
  };
  return (
    <>
      <Button
        size="sm"
        variant="ghost"

        onClick={() => {
          store.refreshExtensions().catch(store.setError.bind(store));
        }}
      >
        Reload extensions
      </Button>
      <label htmlFor={`${fieldPrefix}-field-6`} className="extension-picker">
        Open tool
        <Select
          id={`${fieldPrefix}-field-6`}
          label="Open tool view"
          value={selected}
          onChange={(value) => {
            if (value) {
              choose(value);
            }
          }}
          options={[
            { id: "", label: "Choose a tool…" },
            ...views.map(({ view, extensionId }) => ({
              id: `${extensionId}:${view.id}`,
              label: view.title,
            })),
          ]}
        />
      </label>
      {opened.length > 0 && (
        <div className="open-view-tabs">
          {opened.map((id) => {
            const entry = views.find((value) => `${value.extensionId}:${value.view.id}` === id);
            return (
              entry && (
                <div className="file-tab" key={id}>
                  <Button
                    variant="ghost"
                    size="sm"
                    selected={selected === id}
                    onClick={() => {
                      choose(id);
                    }}
                  >
                    {entry.view.title}
                  </Button>
                  <Button
                    size="sm"
                    variant="ghost"

                    aria-label={`Close ${entry.view.title}`}
                    onClick={() => {
                      close(id);
                    }}
                  >
                    ×
                  </Button>
                </div>
              )
            );
          })}
        </div>
      )}
      {thread.worktreeId ? (
        <>
          {views
            .filter((value) => opened.includes(`${value.extensionId}:${value.view.id}`))
            .map((entry) => {
              const id = `${entry.extensionId}:${entry.view.id}`;
              return (
                <div className="extension-view" key={id} hidden={selected !== id}>
                  {render(entry)}
                </div>
              );
            })}
          {!opened.length && <p className="panel-note">Choose a tool view to open it.</p>}
          {!views.length && <p className="panel-note">No extension views are available.</p>}
        </>
      ) : (
        <p className="panel-note">Open a project to use workspace tools.</p>
      )}
    </>
  );
}
