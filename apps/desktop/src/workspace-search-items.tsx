import type { SearchDialogItem, SearchDialogSection } from "@swarmie/ui-web";
import { File, MessageSquare, Search, Terminal } from "lucide-react";

import { hasConversation } from "~/src/protocol";
import type { Thread } from "~/src/protocol";

export type SearchScope = "all" | "chats" | "files" | "commands";
export type SearchAction = SearchDialogItem & { run: () => void };

export const searchContexts = {
  all: { label: "Search workspace", placeholder: "Search chats, files and commands…" },
  chats: { label: "Search chats", placeholder: "Search chat titles and messages…" },
  files: { label: "Find file", placeholder: "Find a project file…" },
  commands: { label: "Commands", placeholder: "Type a command…" },
} satisfies Record<SearchScope, { label: string; placeholder: string }>;

export function matchesSearch(text: string, query: string) {
  return text.toLowerCase().includes(query.trim().toLowerCase());
}

export function searchNavigation(open: (scope: SearchScope) => void): SearchAction[] {
  return [
    {
      id: "search:chats",
      label: "Search chats",
      icon: <MessageSquare size={16} />,
      run: () => {
        open("chats");
      },
    },
    {
      id: "search:files",
      label: "Find file",
      icon: <File size={16} />,
      run: () => {
        open("files");
      },
    },
    {
      id: "search:commands",
      label: "Search commands",
      icon: <Terminal size={16} />,
      run: () => {
        open("commands");
      },
    },
  ];
}

export function chatSearchSections(threads: Thread[], query: string): SearchDialogSection[] {
  const matches = threads.filter(
    (thread) =>
      hasConversation(thread) &&
      (matchesSearch(thread.title, query) ||
        thread.messages.some((message) => matchesSearch(messageText(message.content), query))),
  );
  const item = (thread: Thread): SearchDialogItem => ({
    id: `chat:${thread.id}`,
    label: thread.title,
    icon: <MessageSquare size={16} />,
    description: chatDescription(thread, query),
  });
  return [
    {
      id: "pinned",
      label: "Pinned chats",
      items: matches.filter((thread) => thread.pinned).map(item),
    },
    {
      id: "chats",
      label: query.trim().length > 0 ? "Chats" : "Recent chats",
      items: matches.filter((thread) => !thread.pinned).map(item),
    },
  ];
}

function messageText(content: Thread["messages"][number]["content"]) {
  return typeof content === "string"
    ? content
    : content
        .filter((part) => part.type === "text")
        .map((part) => part.text)
        .join(" ");
}

function chatDescription(thread: Thread, query: string) {
  if (query.trim().length === 0) {
    return thread.archived === true ? "Archived chat" : "";
  }
  const match = thread.messages
    .map((message) => messageText(message.content))
    .find((text) => matchesSearch(text, query));
  if (match === undefined) {
    return thread.archived === true ? "Archived chat" : "";
  }
  const start = Math.max(0, match.toLowerCase().indexOf(query.trim().toLowerCase()) - 40);
  return `${start > 0 ? "…" : ""}${match.slice(start, start + 180).replaceAll(/\s+/gu, " ")}`;
}

export function fileSearchItems(paths: string[]): SearchDialogItem[] {
  return paths.map((path) => ({ id: `file:${path}`, label: path, icon: <File size={16} /> }));
}

export function scopeSearchSection(actions: SearchAction[], query: string): SearchDialogSection {
  return {
    id: "search",
    label: "Search",
    items: actions
      .filter((action) => matchesSearch(action.label, query))
      .map((action) => ({ ...action, icon: action.icon ?? <Search size={16} /> })),
  };
}
