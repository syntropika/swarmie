import type { ExecutionPolicy, ExecutionProfile } from "@swarmie/contracts";
// Copyright 2026 Syntropika
// SPDX-License-Identifier: Apache-2.0
import { decodeContract } from "@swarmie/contracts/validation";

import type { Thread } from "./protocol";
import { hasText } from "./value-presence";

const unavailable = [
  { profile: "ask", label: "Ask for approval" },
  { profile: "review", label: "Approve for me" },
  { profile: "full", label: "Full access" },
] as const;
export type PermissionMode = Exclude<ExecutionProfile, "native_custom">;

function permissionMode(value: unknown): PermissionMode | undefined {
  return value === "ask" || value === "review" || value === "full" ? value : undefined;
}
function metadataPolicy(value: unknown): ExecutionPolicy | undefined {
  if (value === undefined) {
    return undefined;
  }
  try {
    return decodeContract("ExecutionPolicy", value);
  }
  // Unrecognized metadata cannot make an execution permission available.
  catch {
    return undefined;
  }
}
function policyStatus(policy: ExecutionPolicy | undefined): string | undefined {
  if (policy?.dispatchBlocked === true) {
    return policy.reason ?? "Select a supported policy before continuing";
  }
  if (policy?.status === "staged") {
    return "Selected for the next turn. Current native permissions are shown separately.";
  }
  if (policy?.status === "unknown") {
    return "Current native permissions have not been confirmed.";
  }
  return undefined;
}
function policyOptions(policy: ExecutionPolicy | undefined, thread: Thread) {
  const choices =
    policy?.choices ??
    unavailable.map((choice) => ({
      ...choice,
      description: "Execution permissions have not been described by the server",
      capability: {
        availability: "unknown",
        reason: "Execution permissions have not been described by the server",
      },
      mapping: "unknown",
      application: "next_turn",
    }));
  return choices.flatMap((choice) => {
    const id = permissionMode(choice.profile);
    if (id === undefined) {
      return [];
    }
    return [
      {
        id,
        label: choice.label,
        description: choice.description,
        available:
          choice.capability.availability === "available" && choice.mapping === "equivalent",
        reason:
          choice.capability.reason ??
          (choice.mapping === "equivalent"
            ? undefined
            : "No equivalent native configuration is available"),
        locked:
          hasText(thread.sessionId) &&
          (choice.application === "creation_only" ||
            (thread.state !== "idle" && choice.application !== "current_turn")),
        patch: { executionProfile: choice.profile },
      },
    ];
  });
}
export function sessionPermissions(thread: Thread) {
  const field = thread.configurations?.find((entry) => entry.id === "executionProfile");
  const policy = thread.executionPolicy ?? metadataPolicy(field?.metadata?.policy);
  const draft = hasText(thread.sessionId) ? undefined : thread.config?.executionProfile;
  const selected = requestedPermission(draft, policy);
  const options = policyOptions(policy, thread);
  const label =
    options.find((choice) => choice.id === selected)?.label ??
    (policy?.observed ? "Custom permissions" : "Permissions");
  const observedLabel =
    options.find((choice) => choice.id === policy?.observed?.profile)?.label ??
    (policy?.observed ? "Custom permissions" : undefined);
  return { selected, label, options, status: policyStatus(policy), observedLabel, policy };
}

function requestedPermission(draft: unknown, policy: ExecutionPolicy | undefined) {
  return permissionMode(
    draft ??
      policy?.requested ??
      (policy?.status === "staged" ? policy.plannedProfile : policy?.observed?.profile),
  );
}
