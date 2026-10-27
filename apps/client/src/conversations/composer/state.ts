import type { Conversation, Draft, Execution } from "@swarmie/plugin-conversations-browser";

import { useSelection } from "../../client/use-selection";

export type DeliveryMode = "queue" | "steer";

export const draftText = (draft: Draft) => draft.text;
export const draftAttachments = (draft: Draft) => draft.attachments;
export const draftPermission = (draft: Draft) => draft.permission;
export const draftAccount = (draft: Draft) => draft.account;
export const draftNode = (draft: Draft) => draft.node;
export const executionLabel = (execution: Execution) => execution.label;
export const executionBusy = (execution: Execution) =>
  ["queued", "working", "stopping"].includes(execution.phase);

export function useComposerBusy(chat: Conversation) {
  return useSelection(chat.execution, executionBusy);
}

export function patchDraft(chat: Conversation, change: Partial<Draft>) {
  chat.draft.update((current) => ({ ...current, ...change }));
}

export function isSlashCommand(text: string) {
  return text.startsWith("/") && !text.includes(" ");
}
