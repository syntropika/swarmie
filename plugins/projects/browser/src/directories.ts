/** Paths and cursors are supplied by the node; clients do not infer filesystem authority. */
export type DirectoryPage = {
  path: string;
  parent: string | null;
  directories: { name: string; path: string }[];
  next: string | null;
};

export type DirectoryReader = {
  list: (
    request: { path: string | null; cursor: string | null },
    signal: AbortSignal,
  ) => Promise<DirectoryPage>;
  close: () => void;
};

export type DirectoryBrowser = { open: (nodeId: string) => DirectoryReader };
