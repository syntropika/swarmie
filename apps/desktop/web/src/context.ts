import { defaultUrlTransform } from "react-markdown";

export function markdownUrl(value: string, key: string): string {
  return key === "href" && fileLocation(value)
    ? value
    : defaultUrlTransform(value);
}

export type ContextItem = {
  id: string;
  kind: "file" | "message" | "terminal" | "review" | "resource";
  label: string;
  text: string;
  path?: string;
  startLine?: number;
  endLine?: number;
  sourceId?: string;
  skill?: { name: string; path: string };
};
export type FileLocation = {
  path: string;
  startLine?: number;
  endLine?: number;
};

export function fileLocation(value: string): FileLocation | undefined {
  if (
    /^(?!file:)[a-z][a-z\d+.-]*:\/\//i.test(value) ||
    /^(data|mailto|javascript):/i.test(value)
  )
    return;
  let decoded: string;
  try {
    decoded = decodeURIComponent(value.replace(/^file:\/\//, ""));
  } catch {
    return;
  }
  const match = /^(.*?)(?:#L(\d+)(?:-L?(\d+))?|:(\d+)(?:-(\d+))?)?$/.exec(
    decoded,
  );
  if (!match?.[1] || (!match[1].includes("/") && !/\.[\w-]+$/.test(match[1])))
    return;
  const startLine = Number(match[2] ?? match[4]) || undefined;
  if (
    [match[2], match[3], match[4], match[5]].some(
      (n) =>
        n !== undefined && (!Number.isSafeInteger(Number(n)) || Number(n) < 1),
    )
  )
    return;
  const endLine = Number(match[3] ?? match[5]) || startLine;
  if (startLine && (startLine < 1 || endLine! < startLine)) return;
  return { path: match[1], startLine, endLine };
}

export function mentionAt(text: string, caret: number) {
  const before = text.slice(0, caret);
  const match = /(?:^|\s)@([^\s@]*)$/.exec(before);
  return match
    ? { query: match[1], start: caret - match[1].length - 1, end: caret }
    : undefined;
}

export function lineExcerpt(content: string, start = 1, end?: number): string {
  const lines = content.split("\n");
  if (!Number.isInteger(start) || start < 1 || start > lines.length)
    throw new Error("Choose a line inside the file.");
  if (
    end !== undefined &&
    (!Number.isInteger(end) || end < start || end > lines.length)
  )
    throw new Error("Choose an end line inside the file.");
  return lines.slice(start - 1, end).join("\n");
}

export function contextText(items: ContextItem[]) {
  return items
    .map((item) => `Context (${item.kind}): ${item.label}\n${item.text}`)
    .join("\n\n");
}
