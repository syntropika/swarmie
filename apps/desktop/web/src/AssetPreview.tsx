import { Disclosure } from "@/components/ui/disclosure";
import { lazy, Suspense, useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import type { WorkspaceStore } from "./workspace";
type Asset = {
  name: string;
  mime: string;
  data: string;
  size?: number;
  path?: string;
};
const PdfPreview = lazy(() =>
  import("./PdfPreview").then((module) => ({ default: module.PdfPreview })),
);
export function MediaPreview({
  asset,
  store,
}: {
  asset: Asset;
  store: WorkspaceStore;
}) {
  const [url, setUrl] = useState("");
  const [destination, setDestination] = useState("");
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState("");
  const [error, setError] = useState("");
  useEffect(() => {
    const name = asset.name.replace(/[/\\]/g, "_");
    const dot = name.lastIndexOf(".");
    setDestination(
      dot > 0 ? `${name.slice(0, dot)}-copy${name.slice(dot)}` : `${name}-copy`,
    );
    setSaved("");
    setError("");
  }, [asset.name, asset.data]);
  useEffect(() => {
    const bytes = Uint8Array.from(atob(asset.data), (c) => c.charCodeAt(0));
    const url = URL.createObjectURL(new Blob([bytes], { type: asset.mime }));
    setUrl(url);
    return () => URL.revokeObjectURL(url);
  }, [asset.data, asset.mime]);
  return (
    <div className="asset-preview">
      {["image/png", "image/jpeg", "image/gif", "image/webp"].includes(
        asset.mime,
      ) ? (
        <img src={url} alt={asset.name} />
      ) : asset.mime.startsWith("audio/") ? (
        <audio controls src={url} />
      ) : asset.mime.startsWith("video/") ? (
        <video controls src={url} />
      ) : asset.mime === "application/pdf" ? (
        <Suspense fallback={<p className="muted">Loading PDF…</p>}>
          <PdfPreview data={asset.data} name={asset.name} />
        </Suspense>
      ) : asset.mime.startsWith("text/") ||
        asset.mime === "application/json" ? (
        <pre>
          {new TextDecoder().decode(
            Uint8Array.from(atob(asset.data), (c) => c.charCodeAt(0)),
          )}
        </pre>
      ) : (
        <p className="panel-note">Preview is unavailable for this format.</p>
      )}
      <div className="asset-save-row">
        <Input
          aria-label={`Save path for ${asset.name}`}
          value={destination}
          disabled={saving}
          onChange={(event) => {
            setDestination(event.target.value);
            setSaved("");
          }}
        />
        <Button
          variant="secondary"
          disabled={
            saving || !destination.trim() || !store.active()?.worktreeId
          }
          onClick={() => {
            const thread = store.active();
            if (!thread) return;
            setSaving(true);
            setError("");
            setSaved("");
            void store.invokeExtension<{ path: string }>(
                  "workspace",
                  "assets.export",
                  {
                    projectId: thread.worktreeId,
        ...(thread.sessionId ? { sessionId: thread.sessionId } : {}),
                    path: destination,
                    data: asset.data,
                  },
                )
              .then((result) => setSaved(result.path))
              .catch((error) => setError(String(error)))
              .finally(() => setSaving(false));
          }}
        >
          {saving ? "Saving…" : "Save a copy"}
        </Button>
      </div>
      <p className="panel-note">
        Destination is relative to the current workspace. Existing files are
        preserved.
      </p>
      {saved && (
        <p className="panel-note" role="status">
          Saved to {saved}
        </p>
      )}
      {error && (
        <p className="inline-error" role="alert">
          {error}
        </p>
      )}
    </div>
  );
}
export function OutputAssets({
  store,
  files,
}: {
  store: WorkspaceStore;
  files: string[];
}) {
  const [selected, setSelected] = useState<Asset>();
  const [busy, setBusy] = useState("");
  if (!files.length) return null;
  return (
    <Disclosure className="tool-activity">
      <Disclosure.Summary>Files changed · {files.length}</Disclosure.Summary>
      <div className="output-files">
        {files.map((path) => (
          <Button
            variant="ghost"
            className="small-button"
            key={path}
            disabled={!!busy}
            onClick={() => {
              const thread = store.active();
              if (!thread) return;
              const relative = path.startsWith(`${thread.worktreeId}/`)
                ? path.slice(thread.worktreeId.length + 1)
                : path;
              setBusy(path);
              void store.invokeExtension<Asset>("workspace", "assets.read", {
                    projectId: thread.worktreeId,
        ...(thread.sessionId ? { sessionId: thread.sessionId } : {}),
                    path: relative,
                  })
                .then(setSelected)
                .catch((e) => store.setError(e))
                .finally(() => setBusy(""));
            }}
          >
            {busy === path ? "Reading…" : path}
          </Button>
        ))}
      </div>
      {selected && <MediaPreview asset={selected} store={store} />}
    </Disclosure>
  );
}
