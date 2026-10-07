import { useEffect, useState } from "react";
import { Dialog } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import type { WorkspaceStore } from "./workspace";

export function FileFinder({
  store,
  onClose,
}: {
  store: WorkspaceStore;
  onClose: () => void;
}) {
  const [query, setQuery] = useState("");
  const [files, setFiles] = useState<string[]>([]);
  const [selected, setSelected] = useState(0);
  const [error, setError] = useState("");
  useEffect(() => {
    let cancelled = false;
    const timer = setTimeout(
      () =>
        void store
          .findFiles(query)
          .then((paths) => {
            if (!cancelled) {
              setFiles(paths);
              setSelected(0);
              setError("");
            }
          })
          .catch((e) => {
            if (!cancelled) setError(String(e));
          }),
      120,
    );
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [query, store]);
  const open = (path: string) => {
    store.openFile({ path });
    onClose();
  };
  return (
    <Dialog className="command-palette" label="Find project file" onClose={onClose}>
      <Input
        aria-label="Find project file"
        placeholder="Find a file…"
        autoFocus
        value={query}
        onChange={(e) => setQuery(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "ArrowDown") {
            e.preventDefault();
            setSelected((n) => Math.min(n + 1, files.length - 1));
          }
          if (e.key === "ArrowUp") {
            e.preventDefault();
            setSelected((n) => Math.max(n - 1, 0));
          }
          if (e.key === "Enter" && files[selected]) {
            e.preventDefault();
            open(files[selected]);
          }
        }}
      />
      <div className="command-results">
        {files.map((path, i) => (
          <Button
            key={path}
            variant="ghost"
            className={`command-result ${i === selected ? "selected" : ""}`}
            onClick={() => open(path)}
          >
            {path}
          </Button>
        ))}
        {error ? (
          <p role="alert" className="inline-error">
            {error}
          </p>
        ) : (
          !files.length && (
            <p className="muted">No matching files in this project.</p>
          )
        )}
      </div>
    </Dialog>
  );
}
