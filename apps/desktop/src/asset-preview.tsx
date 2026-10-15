import { lazy, Suspense, useState } from "react";

import { startAction, errorMessage, errorReporter } from "~/src/action-boundary";
import { decodeAsset, decodeExport } from "~/src/asset-values";
import type { PreviewAsset } from "~/src/asset-values";
import { Button } from "~/src/components/ui/button";
import { Disclosure } from "~/src/components/ui/disclosure";
import { Input } from "~/src/components/ui/input";
import { hasText } from "~/src/value-presence";
import type { WorkspaceStore } from "~/src/workspace";

const PdfPreview = lazy(async () => {
  const module = await import("~/src/pdf-preview");
  return { default: module.PdfPreview };
});
function copyName(name: string) {
  const safe = name.replaceAll(/[/\\]/gu, "_");
  const dot = safe.lastIndexOf(".");
  return dot > 0 ? `${safe.slice(0, dot)}-copy${safe.slice(dot)}` : `${safe}-copy`;
}
export function MediaPreview(props: { asset: PreviewAsset; store: WorkspaceStore }) {
  return <MediaContent key={`${props.asset.name}:${props.asset.data}`} {...props} />;
}
function AssetContent({ asset }: { asset: PreviewAsset }) {
  const url = `data:${asset.mime};base64,${asset.data}`;
  if (["image/png", "image/jpeg", "image/gif", "image/webp"].includes(asset.mime)) {
    return <img src={url} alt={asset.name} />;
  }
  if (asset.mime.startsWith("audio/")) {
    return (
      <audio controls src={url}>
        <track kind="captions" label="Captions unavailable" />
      </audio>
    );
  }
  if (asset.mime.startsWith("video/")) {
    return (
      <video controls src={url}>
        <track kind="captions" label="Captions unavailable" />
      </video>
    );
  }
  if (asset.mime === "application/pdf") {
    return (
      <Suspense fallback={<p className="muted">Loading PDF…</p>}>
        <PdfPreview data={asset.data} name={asset.name} />
      </Suspense>
    );
  }
  if (asset.mime.startsWith("text/") || asset.mime === "application/json") {
    return (
      <pre>
        {new TextDecoder().decode(Uint8Array.from(atob(asset.data), (c) => c.codePointAt(0) ?? 0))}
      </pre>
    );
  }
  return <p className="panel-note">Preview is unavailable for this format.</p>;
}
function MediaContent({ asset, store }: { asset: PreviewAsset; store: WorkspaceStore }) {
  const [destination, setDestination] = useState(copyName(asset.name));
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState("");
  const [issue, setIssue] = useState("");
  const save = async () => {
    const thread = store.active();
    if (thread === undefined) {
      return;
    }
    setSaving(true);
    setIssue("");
    setSaved("");
    try {
      const result = await store.invokeExtension(
        "workspace",
        "assets.export",
        {
          projectId: thread.worktreeId,
          ...(hasText(thread.sessionId) ? { sessionId: thread.sessionId } : {}),
          path: destination,
          data: asset.data,
        },
        decodeExport,
      );
      setSaved(result.path);
    } catch (error) {
      setIssue(errorMessage(error));
    } finally {
      setSaving(false);
    }
  };
  return (
    <div className="asset-preview">
      <AssetContent asset={asset} />
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
            saving || destination.trim().length === 0 || !hasText(store.active()?.worktreeId)
          }
          onClick={() => {
            startAction(save(), errorReporter(setIssue));
          }}
        >
          {saving ? "Saving…" : "Save a copy"}
        </Button>
      </div>
      <p className="panel-note">
        Destination is relative to the current workspace. Existing files are preserved.
      </p>
      {hasText(saved) && <output className="panel-note">Saved to {saved}</output>}
      {hasText(issue) && (
        <p className="inline-error" role="alert">
          {issue}
        </p>
      )}
    </div>
  );
}
export function OutputAssets({ store, files }: { store: WorkspaceStore; files: string[] }) {
  const [selected, setSelected] = useState<PreviewAsset>();
  const [busy, setBusy] = useState("");
  const read = async (path: string) => {
    const thread = store.active();
    if (thread === undefined) {
      return;
    }
    const relative = path.startsWith(`${thread.worktreeId}/`)
      ? path.slice(thread.worktreeId.length + 1)
      : path;
    setBusy(path);
    try {
      setSelected(
        await store.invokeExtension(
          "workspace",
          "assets.read",
          {
            projectId: thread.worktreeId,
            ...(hasText(thread.sessionId) ? { sessionId: thread.sessionId } : {}),
            path: relative,
          },
          decodeAsset,
        ),
      );
    } finally {
      setBusy("");
    }
  };
  if (files.length === 0) {
    return null;
  }
  return (
    <div className="tool-activity">
      <Disclosure>
        <Disclosure.Summary>Files changed · {files.length}</Disclosure.Summary>
        <div className="output-files">
          {files.map((path) => (
            <Button
              variant="ghost"
              size="sm"
              key={path}
              disabled={busy.length > 0}
              onClick={() => {
                startAction(read(path), store.setError.bind(store));
              }}
            >
              {busy === path ? "Reading…" : path}
            </Button>
          ))}
        </div>
        {selected !== undefined && <MediaPreview asset={selected} store={store} />}
      </Disclosure>
    </div>
  );
}
