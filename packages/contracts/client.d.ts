// Copyright 2026 Syntropika
// SPDX-License-Identifier: Apache-2.0
import type { RuntimeEvent, SessionSnapshot, SubmissionReceipt, SettingDescriptor, ModelDescriptor, ExecutionPolicy, ConfigurationReceipt, ConfigurationPreview, JsonValue, ProjectRecord, DirectoryResource, PromptQueue } from "./index.js";

/** Native application projection; receipt ownership does not depend on view caches. */
export type SubmissionProjection = {
  entries: { key: string; sessionId: string; receipt: SubmissionReceipt }[];
  unresolved: boolean;
  snapshot: SessionSnapshot;
};
export type SessionProjection = SubmissionProjection & { events: RuntimeEvent[] };
export interface NativeClientMethods {
  "client/sessionProjection": {
    params: { sessionId: string; afterSequence?: number };
    result: SessionProjection;
  };
  "client/submissions": {
    params: { sessionId: string; reconcile?: boolean };
    result: SubmissionProjection;
  };
}

/** User intent for a managed session; directory selection is resolved on its owning node. */
export type SessionLaunchIntent = {
  harness: string; model?: string | null; config?: Record<string, JsonValue>;
  projectId?: string; projectRevision?: number; directoryPath?: string;
};
export type SessionControls = {
  configuration: SettingDescriptor[]; models: ModelDescriptor[];
  values: Record<string, JsonValue>; executionPolicy: ExecutionPolicy;
};
export type SessionOpenProjection = SessionProjection & {
  activation: { status: "attached" | "inactive" | "blocked" | "unavailable"; error?: { code: number; message: string; data?: JsonValue } };
  queue?: PromptQueue;
};
export type ClientProject = {
  id: string; revision: number; name: string; path: string; git: boolean; gitId?: string;
  worktrees: { id: string; path: string; branch: string | null; isMain: boolean; locked: boolean; prunable: boolean }[];
  error?: string;
};
export interface NativeClientMethods {
  "client/sessionCreate": { params: SessionLaunchIntent; result: SessionSnapshot };
  "client/sessionPrepare": { params: SessionLaunchIntent; result: ConfigurationPreview };
  "client/sessionOpen": { params: { sessionId: string; resume?: "active" | "explicit" | "never" }; result: SessionOpenProjection };
  "client/sessionControls": { params: { sessionId: string }; result: SessionControls };
  "client/sessionConfigure": { params: { sessionId: string; config: Record<string, JsonValue> }; result: ConfigurationReceipt };
  "client/projectOpen": { params: { path: string; name?: string }; result: ClientProject };
  "client/projectGet": { params: { projectId: string }; result: ClientProject };
  "client/projectCatalog": { params: Record<string, never>; result: ClientProject[] };
  "client/projectDefaults": { params: { projectId: string }; result: ProjectRecord["launch"] & { cwd?: string; revision: number } };
  "client/chatDirectory": { params: { set?: boolean; path?: string }; result: DirectoryResource };
  "client/extensionInvoke": { params: { extensionId: string; command: string; arguments: JsonValue }; result: JsonValue };
  "client/worktree": { params: { projectId: string; action: "create" | "remove"; branch?: string; worktreeId?: string; sessionId?: string }; result: { tree: ClientProject["worktrees"][number] & { setup?: { completed: boolean; message?: string } } | null; worktrees: ClientProject["worktrees"] } };
}
