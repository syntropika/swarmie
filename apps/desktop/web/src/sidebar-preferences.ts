import { isRecord } from "~/web/src/checked-json";
import { sortedCopy } from "~/web/src/collection-order";
import type { Thread } from "~/web/src/protocol";
// Copyright 2026 Syntropika
// SPDX-License-Identifier: Apache-2.0
import type { SidebarSort } from "~/web/src/sidebar-section";

export type SidebarSorting = Record<"projects" | "recents" | "pinned", SidebarSort>;
function sortPreference(value: unknown, fallback: SidebarSort): SidebarSort {
  return value === "manual" || value === "updated_at" ? value : fallback;
}
export function readSidebarSorting(): SidebarSorting {
  const defaults: SidebarSorting = { projects: "manual", recents: "updated_at", pinned: "manual" };
  try {
    const value: unknown = JSON.parse(localStorage.getItem("swarmie.sidebar-sorting") ?? "null");
    if (!isRecord(value)) {
      return defaults;
    }
    return {
      projects: sortPreference(value.projects, defaults.projects),
      recents: sortPreference(value.recents, defaults.recents),
      pinned: sortPreference(value.pinned, defaults.pinned),
    };
  } catch {
    return defaults;
  }
}
export function sortThreads(threads: Thread[], sort: SidebarSort) {
  return sort === "manual"
    ? threads
    : sortedCopy(
        threads,
        (a, b) => (b.updatedAtMs ?? b.createdAtMs ?? 0) - (a.updatedAtMs ?? a.createdAtMs ?? 0),
      );
}
export function projectActivity(threads: Thread[], id: string) {
  return Math.max(
    0,
    ...threads
      .filter((thread) => thread.projectId === id)
      .map((thread) => thread.updatedAtMs ?? thread.createdAtMs ?? 0),
  );
}
