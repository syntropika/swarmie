import type { ExecutionPolicy, ExecutionProfile } from "@swarmie/contracts";
import type { Thread } from "./protocol";

const unavailable = [
  { profile: "ask", label: "Ask for approval" },
  { profile: "review", label: "Approve for me" },
  { profile: "full", label: "Full access" },
] as const;
export type PermissionMode = Exclude<ExecutionProfile, "native_custom">;

export function sessionPermissions(thread: Thread) {
  const field = thread.configurations?.find((entry) => entry.id === "executionProfile");
  const policy = thread.executionPolicy ?? field?.metadata?.policy as ExecutionPolicy | undefined;
  const draft = !thread.sessionId ? thread.config?.executionProfile : undefined;
  const selected = (draft ?? policy?.requested ?? (policy?.status === "staged" ? policy.plannedProfile : policy?.observed?.profile)) as PermissionMode | undefined;
  const choices = policy?.choices ?? unavailable.map((choice) => ({
    ...choice, description: "Execution permissions have not been described by the server",
    capability: { availability: "unknown", reason: "Execution permissions have not been described by the server" },
    mapping: "unknown", application: "next_turn",
  }));
  const options = choices.filter((choice) => choice.profile !== "native_custom").map((choice) => ({
    id: choice.profile as PermissionMode, label: choice.label, description: choice.description,
    available: choice.capability.availability === "available" && choice.mapping === "equivalent",
    reason: choice.capability.reason ?? (choice.mapping !== "equivalent" ? "No equivalent native configuration is available" : undefined),
    locked: !!thread.sessionId && (choice.application === "creation_only" || (thread.state !== "idle" && choice.application !== "current_turn")),
    patch: { executionProfile: choice.profile },
  }));
  const label = options.find((choice) => choice.id === selected)?.label ?? (policy?.observed ? "Custom permissions" : "Permissions");
  const status = policy?.dispatchBlocked ? policy.reason ?? "Select a supported policy before continuing" :
    policy?.status === "staged" ? "Selected for the next turn. Current native permissions are shown separately." :
    policy?.status === "unknown" ? "Current native permissions have not been confirmed." : undefined;
  const observedLabel = options.find((choice) => choice.id === policy?.observed?.profile)?.label ?? (policy?.observed ? "Custom permissions" : undefined);
  return { selected, label, options, status, observedLabel, policy };
}
