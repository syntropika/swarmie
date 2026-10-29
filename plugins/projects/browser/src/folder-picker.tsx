import { Button, ListBox, ResponsiveDialog, Surface } from "@swarmie/ui-web";
import { ArrowUp, ChevronRight, Folder } from "lucide-react";
import { useEffect, useRef, useState } from "react";

import type { DirectoryBrowser, DirectoryPage, DirectoryReader } from "./directories";

export function FolderPicker({
  browser,
  nodeId,
  choose,
  cancel,
}: {
  browser: DirectoryBrowser;
  nodeId: string;
  choose: (path: string) => void;
  cancel: () => void;
}) {
  const reader = useRef<{
    browser: DirectoryBrowser;
    nodeId: string;
    source: DirectoryReader;
  } | null>(null);
  useEffect(
    () => () => {
      reader.current?.source.close();
      reader.current = null;
    },
    [],
  );
  const [request, setRequest] = useState<{ path: string | null; cursor: string | null }>({
    path: null,
    cursor: null,
  });
  const [state, setState] = useState<{
    page: DirectoryPage | null;
    loading: boolean;
    error: string | null;
  }>({ page: null, loading: true, error: null });
  useEffect(() => {
    let active = reader.current;
    if (active === null || active.browser !== browser || active.nodeId !== nodeId) {
      active?.source.close();
      active = { browser, nodeId, source: browser.open(nodeId) };
      reader.current = active;
    }
    const { source: current } = active;
    const controller = new AbortController();
    async function load() {
      const page = await current.list(request, controller.signal);
      if (!controller.signal.aborted) {
        setState((value) => ({
          page:
            request.cursor !== null && value.page?.path === page.path
              ? { ...page, directories: [...value.page.directories, ...page.directories] }
              : page,
          loading: false,
          error: null,
        }));
      }
    }
    function failed(error: unknown) {
      if (!controller.signal.aborted) {
        setState((value) => ({
          ...value,
          loading: false,
          error: error instanceof Error ? error.message : "The node folder could not be opened.",
        }));
      }
    }
    load().catch(failed);
    return () => {
      controller.abort();
    };
  }, [browser, nodeId, request]);
  const { page, loading, error } = state;
  function navigate(path: string | null, cursor: string | null = null) {
    setState((value) => ({ ...value, loading: true, error: null }));
    setRequest({ path, cursor });
  }
  return (
    <div className="grid gap-4">
      <div className="flex min-w-0 items-center gap-2">
        <Button
          variant="ghost"
          isIconOnly
          aria-label="Parent folder"
          isDisabled={loading || page?.parent === null || page === null}
          onPress={() => {
            if (page !== null && page.parent !== null) {
              navigate(page.parent);
            }
          }}
        >
          <ArrowUp />
        </Button>
        <span className="min-w-0 flex-1 text-sm break-all" aria-label="Current folder">
          {page?.path ?? "Node folders"}
        </span>
      </div>
      <Surface
        variant="secondary"
        aria-label="Node folders"
        aria-busy={loading}
        className="max-h-72 min-h-40 overflow-auto"
      >
        <ListBox aria-label="Folders" selectionMode="none">
          {page?.directories.map((directory) => (
            <ListBox.Item
              key={directory.name}
              id={directory.path}
              textValue={directory.name}
              isDisabled={loading}
              onAction={() => {
                navigate(directory.path);
              }}
            >
              <Folder className="shrink-0" />
              <span className="min-w-0 flex-1 truncate text-left">{directory.name}</span>
              <ChevronRight className="shrink-0" />
            </ListBox.Item>
          ))}
        </ListBox>
        {loading ? (
          <output className="block p-3 text-sm text-muted">Loading folders…</output>
        ) : null}
        {!loading && error === null && page?.directories.length === 0 ? (
          <p className="p-3 text-sm text-muted">No subfolders.</p>
        ) : null}
        {!loading && page !== null && page.next !== null && error === null ? (
          <Button
            variant="tertiary"
            onPress={() => {
              navigate(page.path, page.next);
            }}
          >
            Load more
          </Button>
        ) : null}
      </Surface>
      {error === null ? null : (
        <div className="grid gap-2">
          <p role="alert" className="text-sm text-danger">
            {error}
          </p>
          <Button
            variant="tertiary"
            onPress={() => {
              navigate(request.path, request.cursor);
            }}
          >
            Try again
          </Button>
        </div>
      )}
      <ResponsiveDialog.Footer>
        <Button variant="tertiary" onPress={cancel}>
          Cancel
        </Button>
        <Button
          isDisabled={loading || error !== null || page === null}
          onPress={() => {
            if (page && !loading && error === null) {
              choose(page.path);
            }
          }}
        >
          Select folder
        </Button>
      </ResponsiveDialog.Footer>
    </div>
  );
}
