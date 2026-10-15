import { defaultUrlTransform } from "react-markdown";

import { hasText } from "~/web/src/value-presence";

export function markdownUrl(value: string, key: string): string {
  return key === "href" && fileLocation(value) ? value : defaultUrlTransform(value);
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
    /^(?!file:)[a-z][a-z\d+.-]*:\/\//iu.test(value) ||
    /^(?:data|mailto|javascript):/iu.test(value)
  ) {
    return undefined;
  }
  let decoded: string;
  try {
    decoded = decodeURIComponent(value.replace(/^file:\/\//u, ""));
  } catch {
    return undefined;
  }
  const match =
    /^(?<path>.*?)(?:#L(?<hashStart>\d+)(?:-L?(?<hashEnd>\d+))?|:(?<colonStart>\d+)(?:-(?<colonEnd>\d+))?)?$/u.exec(
      decoded,
    );
  const groups = match?.groups;
  if (
    !groups ||
    !hasText(groups.path) ||
    (!groups.path.includes("/") && !/\.[\w-]+$/u.test(groups.path))
  ) {
    return undefined;
  }
  const { path, hashStart, hashEnd, colonStart, colonEnd } = groups;
  if (
    [hashStart, hashEnd, colonStart, colonEnd].some(
      (n) => n !== undefined && (!Number.isSafeInteger(Number(n)) || Number(n) < 1),
    )
  ) {
    return undefined;
  }
  const startText = hashStart ?? colonStart;
  const endText = hashEnd ?? colonEnd;
  const startLine = startText === undefined ? undefined : Number(startText);
  const endLine = endText === undefined ? startLine : Number(endText);
  if (startLine !== undefined && endLine !== undefined && endLine < startLine) {
    return undefined;
  }
  return { path, startLine, endLine };
}

export function mentionAt(text: string, caret: number) {
  const before = text.slice(0, caret);
  const match = /(?:^|\s)@(?<query>[^\s@]*)$/u.exec(before);
  const query = match?.groups?.query;
  return query === undefined ? undefined : { query, start: caret - query.length - 1, end: caret };
}

export function lineExcerpt(content: string, start = 1, end?: number): string {
  const lines = content.split("\n");
  if (!Number.isInteger(start) || start < 1 || start > lines.length) {
    throw new Error("Choose a line inside the file.");
  }
  if (end !== undefined && (!Number.isInteger(end) || end < start || end > lines.length)) {
    throw new Error("Choose an end line inside the file.");
  }
  return lines.slice(start - 1, end).join("\n");
}

export function contextText(items: ContextItem[]) {
  return items.map((item) => `Context (${item.kind}): ${item.label}\n${item.text}`).join("\n\n");
}
