// Copyright 2026 Syntropika
// SPDX-License-Identifier: Apache-2.0
import type { RuntimeEvent, SessionSnapshot, SubmissionReceipt, SettingDescriptor, ModelDescriptor, ExecutionPolicy, ConfigurationReceipt, ConfigurationPreview, JsonValue, ProjectRecord, DirectoryResource, PromptQueue } from "./index.js";

/** Native application projection; receipt ownership does not depend on view caches. */
export type SubmissionProjection = {
  entries: { key: string; sessionId: string; receipt: SubmissionReceipt; input?: PreparedInput | null }[];
  unresolved: boolean;
  snapshot: SessionSnapshot;
  inputs?: PreparedInput[];
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

/** Authored inputs are interpreted and retained by the application on the selected node. */
export type InputAttachment = { id: string; name: string; mime: string; data: string; image: boolean; binary?: boolean; path?: string; assetId?: string };
export type InputContext = { id: string; kind: "file" | "message" | "terminal" | "review" | "resource"; label: string; text: string; path?: string; startLine?: number; endLine?: number; sourceId?: string; skill?: { name: string; path: string } };
export type PreparedInput = { id?: string; queueId?: string; text: string; attachments: InputAttachment[]; contextItems: InputContext[]; content: import("./index.js").PromptContent[]; afterSequence: number };
export type RecoveredDraft = { draft: string; attachments: InputAttachment[]; contextItems: InputContext[] };
export interface NativeClientMethods {
  "client/sessionRecover": { params: { sessionId: string }; result: SessionOpenProjection };
  "client/sessionInput": { params: { sessionId: string; text: string; attachments?: InputAttachment[]; contextItems?: InputContext[]; delivery?: "prompt" | "steer" | "queue"; idempotencyKey?: string }; result: { input?: PreparedInput; receipt?: SubmissionReceipt; result?: JsonValue; title?: string; metadataError?: JsonValue; queue?: PromptQueue } };
  "client/sessionQueue": { params: { sessionId: string; action: "list" | "update" | "remove" | "pause"; id?: string; text?: string; paused?: boolean }; result: PromptQueue & { input?: PreparedInput } };
  "client/sessionHistory": { params: { sessionId: string; action: "fork" | "branch" | "edit" | "rollback"; messageId?: string; position?: "before_user" | "after_assistant"; turns?: number; scope?: "conversation_only" | "conversation_and_files" }; result: { projection: SessionOpenProjection; draft?: RecoveredDraft | null } };
}

export interface NativeClientMethods {
  "client/projectCreate": { params: { source: "create" | "clone" | "scratch"; arguments?: JsonValue; name?: string }; result: ClientProject };
  "client/gitText": { params: { kind: "commit" | "pr"; arguments: JsonValue; launch: SessionLaunchIntent }; result: { draft: { message?: string; title?: string; body?: string }; truncated: boolean } };
  "client/prProjection": { params: { projectId: string; worktreeId?: string; number?: number }; result: JsonValue };
  "client/fileSave": { params: { extensionId: string; arguments: JsonValue; baseline: string }; result: JsonValue };
  "client/comparison": { params: { launch: SessionLaunchIntent; text: string; isolated?: boolean; targets: { harness: string; model?: string }[] }; result: JsonValue };
  "client/sessionImport": { params: { harness: string; nativeId: string; cwd?: string; configHome?: string; limit?: number }; result: SessionOpenProjection };
}
export interface NativeClientMethods {
  "client/schedulePut": { params: { schedule: { id?: string; sessionId?: string; text: string; nextRunAtMs: number; intervalSeconds?: number; enabled?: boolean }; launch?: SessionLaunchIntent }; result: { created: boolean; session: SessionSnapshot; result: JsonValue } };
}

/** Account-free SSH preparation result; unknown installation is distinct from missing. */
export type SshReadiness = {
  status: "ready" | "ssh-client-unavailable" | "ssh-unavailable" | "not-installed" | "invalid-installation" | "runtime-unavailable" | "incompatible-runtime";
  installed: boolean | null;
  ready: boolean;
  version?: string;
  nextAction: string;
  runtimeIdentity?: import("./index.js").RuntimeIdentity;
};
