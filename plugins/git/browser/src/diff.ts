export type DiffLine = {
  id: string;
  kind: "context" | "added" | "removed" | "header";
  text: string;
  oldLine: number | null;
  newLine: number | null;
};

/** Parse display lines only. The native owner retains the original patch for operations. */
export function diffLines(patch: string): DiffLine[] {
  let oldLine = 0;
  let newLine = 0;
  let inHunk = false;
  return patch
    .split("\n")
    .map((text): Omit<DiffLine, "id"> => {
      const hunk = /^@@ -(?<old>\d+)(?:,\d+)? \+(?<next>\d+)(?:,\d+)? @@/u.exec(text);
      if (hunk) {
        oldLine = Number(hunk.groups?.old);
        newLine = Number(hunk.groups?.next);
        inHunk = true;
        return { kind: "header", text, oldLine: null, newLine: null };
      }
      if (!inHunk || text.startsWith("\\") || text === "") {
        return { kind: "header", text, oldLine: null, newLine: null };
      }
      if (text.startsWith("-")) {
        const previous = oldLine;
        oldLine += 1;
        return { kind: "removed", text: text.slice(1), oldLine: previous, newLine: null };
      }
      if (text.startsWith("+")) {
        const next = newLine;
        newLine += 1;
        return { kind: "added", text: text.slice(1), oldLine: null, newLine: next };
      }
      if (text.startsWith(" ")) {
        const previous = oldLine;
        const next = newLine;
        oldLine += 1;
        newLine += 1;
        return { kind: "context", text: text.slice(1), oldLine: previous, newLine: next };
      }
      inHunk = false;
      return { kind: "header", text, oldLine: null, newLine: null };
    })
    .map((line, position) => ({ ...line, id: `${position}:${line.text}` }));
}

export type SplitLine = { left: DiffLine | null; right: DiffLine | null };

export function splitDiff(lines: readonly DiffLine[]): SplitLine[] {
  const rows: SplitLine[] = [];
  let removed: DiffLine[] = [];
  let added: DiffLine[] = [];
  function flush() {
    for (let index = 0; index < Math.max(removed.length, added.length); index += 1) {
      rows.push({ left: removed[index] ?? null, right: added[index] ?? null });
    }
    removed = [];
    added = [];
  }
  for (const line of lines) {
    if (line.kind === "removed") {
      if (added.length > 0) {
        flush();
      }
      removed.push(line);
    } else if (line.kind === "added") {
      added.push(line);
    } else {
      flush();
      rows.push({ left: line, right: line });
    }
  }
  flush();
  return rows;
}
