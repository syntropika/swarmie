import { useEffect, useRef, useState, useCallback, useId } from "react";

import { MediaPreview } from "~/web/src/asset-preview";
import { attachmentBytes, attachmentLimit } from "~/web/src/attachments";
import { useExtensionQuery } from "~/web/src/components/extension-query";
import { extensionResult } from "~/web/src/components/extension-result";
import { hasText } from "~/web/src/components/presentation-values";
import { Button } from "~/web/src/components/ui/button";
import { Input } from "~/web/src/components/ui/input";
import type { Thread, Attachment } from "~/web/src/protocol";
import type { WorkspaceStore } from "~/web/src/workspace";

type Host = { store: WorkspaceStore; thread: Thread };
type Asset = {
  assetId: string;
  name: string;
  data: string;
  mime: string;
  path: string;
};
async function captureToMessage(store: WorkspaceStore, thread: Thread, assetId: string) {
  const asset = await store.invokeExtension<Asset>(
    "workspace",
    "assets.read",
    {
      assetId,
    },
    extensionResult<Asset>({
      type: "object",
      properties: {
        assetId: { type: "string" },
        name: { type: "string" },
        data: { type: "string" },
        mime: { type: "string" },
        path: { type: "string" },
      },
      required: ["assetId", "name", "data", "mime", "path"],
    }),
  );
  const attachment: Attachment = {
    id: crypto.randomUUID(),
    name: asset.name,
    data: asset.data,
    mime: asset.mime,
    image: ["image/png", "image/jpeg", "image/gif", "image/webp"].includes(asset.mime),
    binary: !asset.mime.startsWith("image/"),
    path: asset.path,
    assetId: asset.assetId,
  };
  const current = store.getSnapshot().threads.find((t) => t.id === thread.id)?.attachments ?? [];
  if (
    current.length >= 8 ||
    [...current, attachment].reduce((sum, a) => sum + attachmentBytes(a), 0) > attachmentLimit
  ) {
    throw new Error("Keep message attachments under eight files and 8 MB.");
  }
  store.updateThread(thread.id, { attachments: [...current, attachment] });
  return asset;
}
function BrowserPreviewContent({ store, thread }: Host) {
  const fieldPrefix = useId();

  const [path, setPath] = useState("");
  const [preview, setPreview] = useState<{
    previewId: string;
    url: string;
    relayPath: string;
  }>();
  const [busy, setBusy] = useState(false);
  const active = useRef("");
  const generation = useRef(0);
  useEffect(() => {
    generation.current += 1;
    return () => {
      generation.current += 1;
      const id = active.current;
      active.current = "";
      if (id) {
        const handlePromiseFailure1 = store.setError.bind(store);

        store
          .invokeExtension("workspace", "preview.stop", {
            previewId: id,
          })
          .catch(handlePromiseFailure1);
      }
    };
  }, [store]);
  const start = async () => {
    const request = generation.current;
    setBusy(true);
    try {
      if (active.current) {
        await store.invokeExtension("workspace", "preview.stop", {
          previewId: active.current,
        });
        active.current = "";
        setPreview(undefined);
      }
      if (request !== generation.current) {
        return;
      }
      const result = await store.invokeExtension<{
        previewId: string;
        url: string;
        relayPath: string;
      }>(
        "workspace",
        "preview.start",
        {
          projectId: thread.worktreeId,
          ...(hasText(thread.sessionId) ? { sessionId: thread.sessionId } : {}),
          path,
        },
        extensionResult<{
          previewId: string;
          url: string;
          relayPath: string;
        }>({
          type: "object",
          properties: {
            previewId: { type: "string" },
            url: { type: "string" },
            relayPath: { type: "string" },
          },
          required: ["previewId", "url", "relayPath"],
        }),
      );
      if (request !== generation.current) {
        await store.invokeExtension("workspace", "preview.stop", {
          previewId: result.previewId,
        });
        return;
      }
      active.current = result.previewId;
      setPreview({
        ...result,
        url: new URL(store.previewPath(result.relayPath), window.location.origin).href,
      });
    } catch (error) {
      store.setError(error);
    } finally {
      if (request === generation.current) {
        setBusy(false);
      }
    }
  };
  return (
    <div className="panel-body preview-panel">
      <h3>Browser preview</h3>
      <label htmlFor={`${fieldPrefix}-field-1`}>
        Project folder
        <Input
          id={`${fieldPrefix}-field-1`}
          value={path}
          onChange={(e) => {
            setPath(e.target.value);
          }}
          placeholder="Project root, or dist"
        />
      </label>
      <div className="permission-actions">
        <Button
          size="sm"
          variant="ghost"

          disabled={busy}
          onClick={() => {
            start().catch(store.setError.bind(store));
          }}
        >
          {preview ? "Restart preview" : "Start preview"}
        </Button>
        {preview && (
          <Button
            size="sm"
            variant="ghost"

            disabled={busy}
            onClick={() => {
              store
                .invokeExtension("workspace", "preview.stop", {
                  previewId: preview.previewId,
                })
                .then(() => {
                  active.current = "";
                  setPreview(undefined);
                })
                .catch(store.setError.bind(store));
            }}
          >
            Stop preview
          </Button>
        )}
      </div>
      {preview && (
        <>
          <a href={preview.url} target="_blank" rel="noreferrer">
            {preview.url}
          </a>
          <iframe
            title="Project browser preview"
            src={preview.url}
            sandbox="allow-scripts allow-forms"
            referrerPolicy="no-referrer"
          />
        </>
      )}
      <p className="panel-note">
        Serves static project files through a scoped daemon preview. Scripts run in an isolated
        frame. Close this view to stop the preview.
      </p>
    </div>
  );
}

type DeviceIntegration = { platform: string; available: boolean; message?: string };
type Device = {
  id: string;
  name: string;
  state: string;
  platform: string;
  kind: string;
};
function DevicesContent({ store, thread }: Host) {
  const [busy, setBusy] = useState(false);
  const [asset, setAsset] = useState<Asset>();
  const [stream, setStream] = useState<{ streamId: string; url: string }>();
  const [observedFrame, setObservedFrame] = useState<{ streamId: string; data: string }>();
  const frame = observedFrame?.streamId === stream?.streamId ? observedFrame?.data : undefined;
  const active = useRef("");
  const generation = useRef(0);
  const owned = useRef<Record<string, string>>({});
  const [emulators, setEmulators] = useState<Record<string, string>>({});
  const [message, setMessage] = useState("");
  const run = useCallback(
    async (action: () => Promise<unknown>) => {
      const request = generation.current;
      setBusy(true);
      try {
        await action();
      } catch (error) {
        store.setError(error);
      } finally {
        if (request === generation.current) {
          setBusy(false);
        }
      }
    },
    [store],
  );
  const fetchDevices = useCallback(async () => {
    const result = await store.invokeExtension<{
      devices: Device[];
      integrations: DeviceIntegration[];
    }>(
      "workspace",
      "devices.list",
      {},
      extensionResult<{
        devices: Device[];
        integrations: DeviceIntegration[];
      }>({
        type: "object",
        properties: {
          devices: {
            type: "array",
            items: {
              type: "object",
              properties: {
                id: { type: "string" },
                name: { type: "string" },
                state: { type: "string" },
                platform: { type: "string" },
                kind: { type: "string" },
              },
              required: ["id", "name", "state", "platform", "kind"],
            },
          },
          integrations: {
            type: "array",
            items: {
              type: "object",
              properties: {
                platform: { type: "string" },
                available: { type: "boolean" },
                message: { type: "string" },
              },
              required: ["platform", "available"],
            },
          },
        },
        required: ["devices", "integrations"],
      }),
    );
    return result;
  }, [store]);
  const catalog = useExtensionQuery(store, ["devices", thread.id], fetchDevices);
  const devices = catalog.data?.devices ?? [];
  const integrations = catalog.data?.integrations ?? [];
  const load = catalog.refresh;
  useEffect(() => {
    if (!stream) {
      return undefined;
    }
    let disposed = false;
    let timer: ReturnType<typeof setTimeout>;
    const refresh = async () => {
      try {
        const result = await store.invokeExtension<{ mime: string; data: string }>(
          "workspace",
          "devices.stream.frame",
          { streamId: stream.streamId },
          extensionResult<{ mime: string; data: string }>({
            type: "object",
            properties: { mime: { type: "string" }, data: { type: "string" } },
            required: ["mime", "data"],
          }),
        );
        if (disposed) {
          return;
        }
        setObservedFrame({
          streamId: stream.streamId,
          data: `data:${result.mime};base64,${result.data}`,
        });
        timer = setTimeout(() => {
          refresh().catch(store.setError.bind(store));
        }, 1000);
      } catch (error) {
        if (!disposed) {
          setMessage(
            `Live view interrupted: ${String(error)}. Stop and start the live view to reconnect.`,
          );
        }
      }
    };
    refresh().catch(store.setError.bind(store));
    return () => {
      disposed = true;
      clearTimeout(timer);
    };
  }, [stream, store]);
  useEffect(() => {
    generation.current += 1;
    return () => {
      generation.current += 1;
      for (const terminalId of Object.values(owned.current)) {
        const handlePromiseFailure7 = store.setError.bind(store);

        store
          .invokeExtension("workspace", "terminal.close", { terminalId })
          .catch(handlePromiseFailure7);
      }
      owned.current = {};
      if (active.current) {
        const handlePromiseFailure8 = store.setError.bind(store);

        store
          .invokeExtension("workspace", "devices.stream.stop", {
            streamId: active.current,
          })
          .catch(handlePromiseFailure8);
      }
    };
  }, [store]);

  useEffect(() => {
    if (!Object.keys(emulators).length) {
      return undefined;
    }
    let disposed = false;
    let pending = false;
    const timer = setInterval(() => {
      if (pending) {
        return;
      }
      pending = true;
      Promise.all(
        Object.entries(emulators).map(async ([deviceId, terminalId]) => {
          try {
            const result = await store.invokeExtension<{
              exited: boolean;
              exitCode?: number;
              text: string;
            }>(
              "workspace",
              "terminal.read",
              { terminalId, after: 0 },
              extensionResult<{
                exited: boolean;
                exitCode?: number;
                text: string;
              }>({
                type: "object",
                properties: {
                  exited: { type: "boolean" },
                  exitCode: { type: "number" },
                  text: { type: "string" },
                },
                required: ["exited", "text"],
              }),
            );
            if (disposed || owned.current[deviceId] !== terminalId || !result.exited) {
              return;
            }
            await store.invokeExtension("workspace", "terminal.close", {
              terminalId,
            });
            if (disposed || owned.current[deviceId] !== terminalId) {
              return;
            }
            let remaining = { ...owned.current };
            remaining = Object.fromEntries(
              Object.entries(remaining).filter(([entryKey]) => entryKey !== deviceId),
            );
            owned.current = remaining;
            setEmulators(remaining);
            setMessage(
              `Emulator exited with code ${result.exitCode ?? "unknown"}. ${result.text.slice(-1200)}`,
            );
          } catch (error) {
            if (disposed || owned.current[deviceId] !== terminalId) {
              return;
            }
            let remaining = { ...owned.current };
            remaining = Object.fromEntries(
              Object.entries(remaining).filter(([entryKey]) => entryKey !== deviceId),
            );
            owned.current = remaining;
            setEmulators(remaining);
            setMessage(`Emulator status is unavailable: ${String(error)}`);
          }
        }),
      )
        .finally(() => {
          pending = false;
        })
        .catch(store.setError.bind(store));
    }, 1000);
    return () => {
      disposed = true;
      clearInterval(timer);
    };
  }, [emulators, store]);
  return (
    <div className="panel-body">
      <h3>Simulators and devices</h3>
      {message && <output className="panel-note">{message}</output>}
      <p className="panel-note">
        Emulators launched here stop when this tool view closes. Existing devices and iOS simulators
        remain managed by their native applications.
      </p>
      <Button
        size="sm"
        variant="ghost"

        disabled={busy}
        onClick={() => {
          run(load).catch(store.setError.bind(store));
        }}
      >
        Refresh devices
      </Button>
      {integrations
        .filter((i) => !i.available)
        .map((i) => (
          <p className="panel-note" key={i.platform}>
            {i.platform}: {i.message}
          </p>
        ))}
      {devices.map((device) => (
        <div className="device-row" key={`${device.platform}:${device.id}`}>
          <strong>{device.name}</strong>
          <p className="muted">
            {device.platform} · {device.kind} · {device.state}
          </p>
          <div className="permission-actions">
            <Button
              size="sm"
              variant="ghost"

              disabled={busy || !!emulators[device.id]}
              onClick={() => {
                run(async () => {
                  const request = generation.current;
                  const result = await store.invokeExtension<{
                    message?: string;
                    terminalId?: string;
                  }>(
                    "workspace",
                    "devices.open",
                    {
                      platform: device.platform,
                      id: device.id,
                    },
                    extensionResult<{
                      message?: string;
                      terminalId?: string;
                    }>({
                      type: "object",
                      properties: { message: { type: "string" }, terminalId: { type: "string" } },
                      required: [],
                    }),
                  );
                  if (request !== generation.current) {
                    if (hasText(result.terminalId)) {
                      await store.invokeExtension("workspace", "terminal.close", {
                        terminalId: result.terminalId,
                      });
                    }
                    return;
                  }
                  if (hasText(result.terminalId)) {
                    owned.current = {
                      ...owned.current,
                      [device.id]: result.terminalId,
                    };
                    setEmulators(owned.current);
                  }
                  setMessage(result.message ?? "Device opened.");
                  await load();
                }).catch(store.setError.bind(store));
              }}
            >
              Open device
            </Button>
            {emulators[device.id] && (
              <Button
                size="sm"
                variant="ghost"

                disabled={busy}
                onClick={() => {
                  run(async () => {
                    await store.invokeExtension("workspace", "terminal.close", {
                      terminalId: emulators[device.id],
                    });
                    let remaining = { ...owned.current };
                    remaining = Object.fromEntries(
                      Object.entries(remaining).filter(([entryKey]) => entryKey !== device.id),
                    );
                    owned.current = remaining;
                    setEmulators(remaining);
                  }).catch(store.setError.bind(store));
                }}
              >
                Stop emulator started here
              </Button>
            )}
            <Button
              size="sm"
              variant="ghost"

              disabled={busy}
              onClick={() => {
                run(async () => {
                  const captured = await store.invokeExtension<{
                    assetId: string;
                  }>(
                    "workspace",
                    "devices.capture",
                    {
                      platform: device.platform,
                      id: device.id,
                    },
                    extensionResult<{
                      assetId: string;
                    }>({
                      type: "object",
                      properties: { assetId: { type: "string" } },
                      required: ["assetId"],
                    }),
                  );
                  setAsset(await captureToMessage(store, thread, captured.assetId));
                }).catch(store.setError.bind(store));
              }}
            >
              Capture to message
            </Button>
            <Button
              size="sm"
              variant="ghost"

              disabled={busy}
              onClick={() => {
                run(async () => {
                  const request = generation.current;
                  if (active.current) {
                    await store.invokeExtension("workspace", "devices.stream.stop", {
                      streamId: active.current,
                    });
                  }
                  const started = await store.invokeExtension<{
                    streamId: string;
                    url: string;
                  }>(
                    "workspace",
                    "devices.stream",
                    {
                      platform: device.platform,
                      id: device.id,
                      intervalMs: 1000,
                    },
                    extensionResult<{
                      streamId: string;
                      url: string;
                    }>({
                      type: "object",
                      properties: { streamId: { type: "string" }, url: { type: "string" } },
                      required: ["streamId", "url"],
                    }),
                  );
                  if (request !== generation.current) {
                    await store.invokeExtension("workspace", "devices.stream.stop", {
                      streamId: started.streamId,
                    });
                    return;
                  }
                  active.current = started.streamId;
                  setStream(started);
                }).catch(store.setError.bind(store));
              }}
            >
              Live view
            </Button>
          </div>
        </div>
      ))}
      {stream && (
        <>
          {hasText(frame) ? (
            <img className="device-stream" src={frame} alt="Live device screen" />
          ) : (
            <p className="panel-note">Waiting for a device frame…</p>
          )}
          <Button
            size="sm"
            variant="ghost"

            disabled={busy}
            onClick={() => {
              run(async () => {
                await store.invokeExtension("workspace", "devices.stream.stop", {
                  streamId: stream.streamId,
                });
                active.current = "";
                setStream(undefined);
              }).catch(store.setError.bind(store));
            }}
          >
            Stop live view
          </Button>
        </>
      )}
      {asset && <MediaPreview asset={asset} store={store} />}
      {!devices.length && !busy && (
        <p className="panel-note">
          No devices found. Start an installed simulator or connect a device.
        </p>
      )}
    </div>
  );
}
export function SystemWindows({ store, thread }: Host) {
  const [windows, setWindows] = useState<{ id: string; title: string; application?: string }[]>([]);
  const [busy, setBusy] = useState(false);
  const [context, setContext] = useState<unknown>();
  const [asset, setAsset] = useState<Asset>();
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
  return (
    <div className="panel-body">
      <h3>System windows</h3>
      <Button
        size="sm"
        variant="ghost"

        disabled={busy}
        onClick={() => {
          run(async () => {
            const result = await store.invokeExtension<{
              windows: typeof windows;
            }>(
              "workspace",
              "windows.list",
              {},
              extensionResult<{
                windows: typeof windows;
              }>({
                type: "object",
                properties: {
                  windows: {
                    type: "array",
                    items: {
                      type: "object",
                      properties: {
                        id: { type: "string" },
                        title: { type: "string" },
                        application: { type: "string" },
                      },
                      required: ["id", "title"],
                    },
                  },
                },
                required: ["windows"],
              }),
            );
            setWindows(result.windows);
          }).catch(store.setError.bind(store));
        }}
      >
        Find windows
      </Button>
      {windows.map((window) => (
        <div className="device-row" key={window.id}>
          <strong>{window.title || hasText(window.application) || window.id}</strong>
          <div className="permission-actions">
            <Button
              size="sm"
              variant="ghost"

              disabled={busy}
              onClick={() => {
                run(async () => {
                  const captured = await store.invokeExtension<{
                    assetId: string;
                  }>(
                    "workspace",
                    "windows.capture",
                    { windowId: window.id },
                    extensionResult<{
                      assetId: string;
                    }>({
                      type: "object",
                      properties: { assetId: { type: "string" } },
                      required: ["assetId"],
                    }),
                  );
                  setAsset(await captureToMessage(store, thread, captured.assetId));
                }).catch(store.setError.bind(store));
              }}
            >
              Capture to message
            </Button>
            <Button
              size="sm"
              variant="ghost"

              disabled={busy}
              onClick={() => {
                run(async () => {
                  const result = await store.invokeExtension("workspace", "windows.context", {
                    windowId: window.id,
                  });
                  setContext(result);
                  store.addContext(thread.id, {
                    kind: "message",
                    label: `Window · ${window.title || hasText(window.application) || window.id}`,
                    sourceId: window.id,
                    text: JSON.stringify(result, null, 2),
                  });
                }).catch(store.setError.bind(store));
              }}
            >
              Add accessibility context
            </Button>
          </div>
        </div>
      ))}
      {context !== undefined && (
        <pre className="action-output">{JSON.stringify(context, null, 2)}</pre>
      )}
      {asset && <MediaPreview asset={asset} store={store} />}
      <p className="panel-note">
        Uses native window and accessibility APIs. The operating system may require screen capture
        or accessibility permission.
      </p>
    </div>
  );
}

export function BrowserPreview(props: Host) {
  return (
    <BrowserPreviewContent
      key={`${props.thread.id}:${props.thread.worktreeId}:${props.thread.sessionId ?? ""}`}
      {...props}
    />
  );
}
export function Devices(props: Host) {
  return (
    <DevicesContent
      key={`${props.thread.id}:${props.thread.worktreeId}:${props.thread.sessionId ?? ""}`}
      {...props}
    />
  );
}
