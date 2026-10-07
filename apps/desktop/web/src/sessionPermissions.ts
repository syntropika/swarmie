import type { Thread } from "./protocol";

export const permissionModes = [
  { id: "ask", label: "Ask for approval", description: "Ask before actions outside the workspace or using the internet" },
  { id: "review", label: "Approve for me", description: "Automatically review requests and ask about potentially unsafe actions" },
  { id: "full", label: "Full access", description: "Unrestricted access to the internet and files on your computer" },
] as const;
export type PermissionMode = typeof permissionModes[number]["id"];

export function sessionPermissions(thread: Thread) {
  const fields = thread.configurations ?? [];
  const field = (id: string) => fields.find((entry) => entry.id === id);
  const value = (id: string) => (!thread.sessionId ? thread.config?.[id] : undefined) ?? field(id)?.value;
  const supports = (id: string, next: string) => !!field(id)?.choices?.some((choice) => choice.id === next);
  const codex = !!field("approvalPolicy");
  const reviewer = field("approvalsReviewer");
  const patches: Record<PermissionMode, Record<string, unknown>> = codex ? {
    ask: { approvalPolicy: "on-request", sandbox: "workspace-write", ...(reviewer ? { approvalsReviewer: "user" } : {}) },
    review: { approvalPolicy: "on-request", sandbox: "workspace-write", approvalsReviewer: "auto_review" },
    full: { approvalPolicy: "never", sandbox: "danger-full-access", ...(reviewer ? { approvalsReviewer: "user" } : {}) },
  } : { ask: { permissionMode: "default" }, review: {}, full: { permissionMode: "bypassPermissions" } };
  let selected: PermissionMode | undefined;
  if (codex) {
    if (value("approvalPolicy") === "never" && value("sandbox") === "danger-full-access" && (!reviewer || value("approvalsReviewer") === "user")) selected = "full";
    if (value("approvalPolicy") === "on-request" && value("sandbox") === "workspace-write") {
      if (!reviewer || value("approvalsReviewer") === "user") selected = "ask";
      else if (value("approvalsReviewer") === "auto_review") selected = "review";
    }
  } else if (value("permissionMode") === "default") selected = "ask";
  else if (value("permissionMode") === "bypassPermissions") selected = "full";
  const options = permissionModes.map((mode) => {
    const patch = patches[mode.id];
    const available = Object.keys(patch).length > 0 && Object.entries(patch).every(([id, next]) => supports(id, String(next)));
    const locked = !!thread.sessionId && Object.keys(patch).some((id) => !field(id)?.live);
    return { ...mode, patch, available, locked };
  });
  return { selected, label: permissionModes.find((mode) => mode.id === selected)?.label ?? (fields.length ? "Custom permissions" : "Permissions"), options };
}
