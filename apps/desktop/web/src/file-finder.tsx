import { useEffect, useState } from "react";

import { startAction, errorMessage } from "~/web/src/action-boundary";
import { Button } from "~/web/src/components/ui/button";
import { Dialog } from "~/web/src/components/ui/dialog";
import { Input } from "~/web/src/components/ui/input";
import type { WorkspaceStore } from "~/web/src/workspace";

export function FileFinder({ store, onClose }: { store: WorkspaceStore; onClose: () => void }) {
  const [query, setQuery] = useState("");
  const [files, setFiles] = useState<string[]>([]);
  const [selected, setSelected] = useState(0);
  const [issue, setIssue] = useState("");
  useEffect(() => {
    let cancelled = false;
    const search = async () => {
      try {
        const paths = await store.findFiles(query);
        if (!cancelled) {
          setFiles(paths);
          setSelected(0);
          setIssue("");
        }
      } catch (error) {
        if (!cancelled) {
          setIssue(errorMessage(error));
        }
      }
    };
    const timer = setTimeout(() => {
      startAction(search(), store.setError.bind(store));
    }, 120);

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
    <Dialog variant="command-palette" label="Find project file" onClose={onClose}>
      <Input
        aria-label="Find project file"
        placeholder="Find a file…"
        autoFocus
        value={query}
        onChange={(e) => {
          setQuery(e.target.value);
        }}
        onKeyDown={(e) => {
          if (e.key === "ArrowDown") {
            e.preventDefault();
            setSelected((n) => Math.min(n + 1, files.length - 1));
          }
          if (e.key === "ArrowUp") {
            e.preventDefault();
            setSelected((n) => Math.max(n - 1, 0));
          }
          const path = files.at(selected);
          if (e.key === "Enter" && path !== undefined) {
            e.preventDefault();
            open(path);
          }
        }}
      />
      <div className="grid max-h-96 gap-1 overflow-y-auto">
        {files.map((path, i) => (
          <Button
            key={path}
            variant={i === selected ? "secondary" : "command"}
            className="w-full"
            onClick={() => {
              open(path);
            }}
          >
            {path}
          </Button>
        ))}
        {issue ? (
          <p role="alert" className="inline-error">
            {issue}
          </p>
        ) : (
          !files.length && <p className="muted">No matching files in this project.</p>
        )}
      </div>
    </Dialog>
  );
}
