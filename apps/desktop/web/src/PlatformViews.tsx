import { useEffect, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import type { Thread, Attachment } from "./protocol";
import type { WorkspaceStore } from "./workspace";
import { MediaPreview } from "./AssetPreview";
import { attachmentBytes, attachmentLimit } from "./attachments";
type Host = { store: WorkspaceStore; thread: Thread };
type Asset = {
  assetId: string;
  name: string;
  data: string;
  mime: string;
  path: string;
};
async function captureToMessage(
  store: WorkspaceStore,
  thread: Thread,
  assetId: string,
) {
  const asset = await store.invokeExtension<Asset>("workspace", "assets.read", {
    assetId,
  });
  const attachment: Attachment = {
    id: crypto.randomUUID(),
    name: asset.name,
    data: asset.data,
    mime: asset.mime,
    image: ["image/png", "image/jpeg", "image/gif", "image/webp"].includes(
      asset.mime,
    ),
    binary: !asset.mime.startsWith("image/"),
    path: asset.path,
    assetId: asset.assetId,
  };
  const current =
    store.getSnapshot().threads.find((t) => t.id === thread.id)?.attachments ??
    [];
  if (
    current.length >= 8 ||
    [...current, attachment].reduce((sum, a) => sum + attachmentBytes(a), 0) >
      attachmentLimit
  )
    throw new Error("Keep message attachments under eight files and 8 MB.");
  store.updateThread(thread.id, { attachments: [...current, attachment] });
  return asset;
}
export function BrowserPreview({ store, thread }: Host) {
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
    generation.current++;
    setPreview(undefined);
    setBusy(false);
    return () => {
      generation.current++;
      const id = active.current;
      active.current = "";
      if (id)
        void store
          .invokeExtension("workspace", "preview.stop", {
            previewId: id,
          })
          .catch(() => {});
    };
  }, [thread.id, thread.worktreeId]);
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
      if (request !== generation.current) return;
      const result = await store.invokeExtension<{
        previewId: string;
        url: string;
        relayPath: string;
      }>("workspace", "preview.start", { projectId: thread.worktreeId,
        ...(thread.sessionId ? { sessionId: thread.sessionId } : {}), path });
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
    } catch (e) {
      store.setError(e);
    } finally {
      if (request === generation.current) setBusy(false);
    }
  };
  return (
    <div className="panel-body preview-panel">
      <h3>Browser preview</h3>
      <label>
        Project folder
        <Input
          value={path}
          onChange={(e) => setPath(e.target.value)}
          placeholder="Project root, or dist"
        />
      </label>
      <div className="permission-actions">
        <Button
          variant="ghost"
          className="small-button"
          disabled={busy}
          onClick={() => void start()}
        >
          {preview ? "Restart preview" : "Start preview"}
        </Button>
        {preview && (
          <Button
            variant="ghost"
            className="small-button"
            disabled={busy}
            onClick={() => {
              void store
                .invokeExtension("workspace", "preview.stop", {
                  previewId: preview.previewId,
                })
                .then(() => {
                  active.current = "";
                  setPreview(undefined);
                })
                .catch((e) => store.setError(e));
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
        Serves static project files through a scoped daemon preview. Scripts run
        in an isolated frame. Close this view to stop the preview.
      </p>
    </div>
  );
}

type Device = {
  id: string;
  name: string;
  state: string;
  platform: string;
  kind: string;
};
export function Devices({ store, thread }: Host) {
  const [devices, setDevices] = useState<Device[]>([]);
  const [integrations, setIntegrations] = useState<
    { platform: string; available: boolean; message?: string }[]
  >([]);
  const [busy, setBusy] = useState(false);
  const [asset, setAsset] = useState<Asset>();
  const [stream, setStream] = useState<{ streamId: string; url: string }>();
  const [frame, setFrame] = useState<string>();
  const active = useRef("");
  const generation = useRef(0);
  const owned = useRef<Record<string, string>>({});
  const [emulators, setEmulators] = useState<Record<string, string>>({});
  const [message, setMessage] = useState("");
  const run = async (action: () => Promise<unknown>) => {
    const request = generation.current;
    setBusy(true);
    try {
      await action();
    } catch (e) {
      store.setError(e);
    } finally {
      if (request === generation.current) setBusy(false);
    }
  };
  const load = async () => {
    const request = generation.current;
    const result = await store.invokeExtension<{
      devices: Device[];
      integrations: typeof integrations;
    }>("workspace", "devices.list", {});
    if (request !== generation.current) return;
    setDevices(result.devices);
    setIntegrations(result.integrations);
  };
  useEffect(() => {
    setFrame(undefined);
    if (!stream) return;
    let disposed = false;
    let timer: ReturnType<typeof setTimeout>;
    const refresh = async () => {
      try {
        const result = await store.invokeExtension<{ mime: string; data: string }>(
          "workspace", "devices.stream.frame", { streamId: stream.streamId },
        );
        if (disposed) return;
        setFrame(`data:${result.mime};base64,${result.data}`);
        timer = setTimeout(() => void refresh(), 1000);
      } catch (error) {
        if (!disposed) setMessage(`Live view interrupted: ${String(error)}. Stop and start the live view to reconnect.`);
      }
    };
    void refresh();
    return () => { disposed = true; clearTimeout(timer); };
  }, [stream?.streamId, store]);
  useEffect(() => {
    generation.current++;
    void run(load);
    return () => {
      generation.current++;
      for (const terminalId of Object.values(owned.current))
        void store
          .invokeExtension("workspace", "terminal.close", { terminalId })
          .catch(() => {});
      owned.current = {};
      if (active.current)
        void store
          .invokeExtension("workspace", "devices.stream.stop", {
            streamId: active.current,
          })
          .catch(() => {});
    };
  }, []);

  useEffect(() => {
    if (!Object.keys(emulators).length) return;
    let disposed = false,
      pending = false;
    const timer = setInterval(() => {
      if (pending) return;
      pending = true;
      void Promise.all(
        Object.entries(emulators).map(async ([deviceId, terminalId]) => {
          try {
            const result = await store.invokeExtension<{
              exited: boolean;
              exitCode?: number;
              text: string;
            }>("workspace", "terminal.read", { terminalId, after: 0 });
            if (
              disposed ||
              owned.current[deviceId] !== terminalId ||
              !result.exited
            )
              return;
            await store.invokeExtension("workspace", "terminal.close", {
              terminalId,
            });
            if (disposed || owned.current[deviceId] !== terminalId) return;
            const remaining = { ...owned.current };
            delete remaining[deviceId];
            owned.current = remaining;
            setEmulators(remaining);
            setMessage(
              `Emulator exited with code ${result.exitCode ?? "unknown"}. ${result.text.slice(-1200)}`,
            );
          } catch (error) {
            if (disposed || owned.current[deviceId] !== terminalId) return;
            const remaining = { ...owned.current };
            delete remaining[deviceId];
            owned.current = remaining;
            setEmulators(remaining);
            setMessage(`Emulator status is unavailable: ${String(error)}`);
          }
        }),
      ).finally(() => {
        pending = false;
      });
    }, 1000);
    return () => {
      disposed = true;
      clearInterval(timer);
    };
  }, [emulators]);
  return (
    <div className="panel-body">
      <h3>Simulators and devices</h3>
      {message && (
        <p className="panel-note" role="status">
          {message}
        </p>
      )}
      <p className="panel-note">
        Emulators launched here stop when this tool view closes. Existing
        devices and iOS simulators remain managed by their native applications.
      </p>
      <Button
        variant="ghost"
        className="small-button"
        disabled={busy}
        onClick={() => void run(load)}
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
              variant="ghost"
              className="small-button"
              disabled={busy || !!emulators[device.id]}
              onClick={() =>
                void run(async () => {
                  const request = generation.current;
                  const result = await store.invokeExtension<{
                    message?: string;
                    terminalId?: string;
                  }>("workspace", "devices.open", {
                    platform: device.platform,
                    id: device.id,
                  });
                  if (request !== generation.current) {
                    if (result.terminalId)
                      await store.invokeExtension(
                        "workspace",
                        "terminal.close",
                        { terminalId: result.terminalId },
                      );
                    return;
                  }
                  if (result.terminalId) {
                    owned.current = {
                      ...owned.current,
                      [device.id]: result.terminalId,
                    };
                    setEmulators(owned.current);
                  }
                  setMessage(result.message ?? "Device opened.");
                  await load();
                })
              }
            >
              Open device
            </Button>
            {emulators[device.id] && (
              <Button
                variant="ghost"
                className="small-button"
                disabled={busy}
                onClick={() =>
                  void run(async () => {
                    await store.invokeExtension("workspace", "terminal.close", {
                      terminalId: emulators[device.id],
                    });
                    const remaining = { ...owned.current };
                    delete remaining[device.id];
                    owned.current = remaining;
                    setEmulators(remaining);
                  })
                }
              >
                Stop emulator started here
              </Button>
            )}
            <Button
              variant="ghost"
              className="small-button"
              disabled={busy}
              onClick={() =>
                void run(async () => {
                  const captured = await store.invokeExtension<{
                    assetId: string;
                  }>("workspace", "devices.capture", {
                    platform: device.platform,
                    id: device.id,
                  });
                  setAsset(
                    await captureToMessage(store, thread, captured.assetId),
                  );
                })
              }
            >
              Capture to message
            </Button>
            <Button
              variant="ghost"
              className="small-button"
              disabled={busy}
              onClick={() =>
                void run(async () => {
                  const request = generation.current;
                  if (active.current)
                    await store.invokeExtension(
                      "workspace",
                      "devices.stream.stop",
                      { streamId: active.current },
                    );
                  const started = await store.invokeExtension<{
                    streamId: string;
                    url: string;
                  }>("workspace", "devices.stream", {
                    platform: device.platform,
                    id: device.id,
                    intervalMs: 1000,
                  });
                  if (request !== generation.current) {
                    await store.invokeExtension(
                      "workspace",
                      "devices.stream.stop",
                      { streamId: started.streamId },
                    );
                    return;
                  }
                  active.current = started.streamId;
                  setStream(started);
                })
              }
            >
              Live view
            </Button>
          </div>
        </div>
      ))}
      {stream && (
        <>
          {frame ? <img
            className="device-stream"
            src={frame}
            alt="Live device screen"
          /> : <p className="panel-note">Waiting for a device frame…</p>}
          <Button
            variant="ghost"
            className="small-button"
            disabled={busy}
            onClick={() =>
              void run(async () => {
                await store.invokeExtension(
                  "workspace",
                  "devices.stream.stop",
                  { streamId: stream.streamId },
                );
                active.current = "";
                setStream(undefined);
              })
            }
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
  const [windows, setWindows] = useState<
    { id: string; title: string; application?: string }[]
  >([]);
  const [busy, setBusy] = useState(false);
  const [context, setContext] = useState<unknown>();
  const [asset, setAsset] = useState<Asset>();
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
  return (
    <div className="panel-body">
      <h3>System windows</h3>
      <Button
        variant="ghost"
        className="small-button"
        disabled={busy}
        onClick={() =>
          void run(async () => {
            const result = await store.invokeExtension<{
              windows: typeof windows;
            }>("workspace", "windows.list", {});
            setWindows(result.windows);
          })
        }
      >
        Find windows
      </Button>
      {windows.map((window) => (
        <div className="device-row" key={window.id}>
          <strong>{window.title || window.application || window.id}</strong>
          <div className="permission-actions">
            <Button
              variant="ghost"
              className="small-button"
              disabled={busy}
              onClick={() =>
                void run(async () => {
                  const captured = await store.invokeExtension<{
                    assetId: string;
                  }>("workspace", "windows.capture", { windowId: window.id });
                  setAsset(
                    await captureToMessage(store, thread, captured.assetId),
                  );
                })
              }
            >
              Capture to message
            </Button>
            <Button
              variant="ghost"
              className="small-button"
              disabled={busy}
              onClick={() =>
                void run(async () => {
                  const result = await store.invokeExtension(
                    "workspace",
                    "windows.context",
                    { windowId: window.id },
                  );
                  setContext(result);
                  store.addContext(thread.id, {
                    kind: "message",
                    label: `Window · ${window.title || window.application || window.id}`,
                    sourceId: window.id,
                    text: JSON.stringify(result, null, 2),
                  });
                })
              }
            >
              Add accessibility context
            </Button>
          </div>
        </div>
      ))}
      {context != null && (
        <pre className="action-output">{JSON.stringify(context, null, 2)}</pre>
      )}
      {asset && <MediaPreview asset={asset} store={store} />}
      <p className="panel-note">
        Uses native window and accessibility APIs. The operating system may
        require screen capture or accessibility permission.
      </p>
    </div>
  );
}
