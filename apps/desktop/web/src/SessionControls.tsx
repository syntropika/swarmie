import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { useEffect, useState } from "react";
import { ChevronDown, Shield, Zap } from "lucide-react";
import { Button } from "@/components/ui/button";
import type { Thread } from "./protocol";
import type { WorkspaceStore } from "./workspace";
import { ReasoningEffort } from "./ReasoningEffort";
import { ModelPicker } from "./ModelPicker";
import { PermissionPicker } from "./PermissionPicker";
import { sessionPermissions } from "./sessionPermissions";
import { Tooltip, TooltipContent } from "@/components/ui/tooltip";

export function SessionControls({
  thread,
  store,
  connected,
  kind = "model",
}: {
  thread: Thread;
  store: WorkspaceStore;
  connected: boolean;
  kind?: "model" | "settings";
}) {
  const settings = kind === "settings";
  const [open, setOpen] = useState(false);
  const [loading, setLoading] = useState(false);
  const [previewError, setPreviewError] = useState("");
  const configKey = JSON.stringify(thread.config ?? {});
  useEffect(() => {
    if (!connected || !thread.worktreeId || thread.state !== "idle") {
      setLoading(false);
      setPreviewError("");
      return;
    }
    let current = true;
    setLoading(true);
    setPreviewError("");
    void store.previewControls(thread.id).catch((error) => {
      const message = error instanceof Error ? error.message : String(error);
      if (current && message.toLowerCase() !== "session is busy") setPreviewError(message);
    }).finally(() => { if (current) setLoading(false); });
    return () => { current = false; };
  }, [store, connected, thread.id, thread.harness, thread.worktreeId, thread.model, thread.sessionId, thread.state, configKey]);
  const modelOption = thread.configurations?.find((option) => option.id === "model");
  const models = modelOption?.choices?.length ? modelOption.choices : thread.models ?? [];
  const model = String(thread.model || modelOption?.value || "");
  const modelLabel = models.find((item) => item.id === model)?.label || model || thread.actualModel || "Select model";
  const effort = thread.configurations?.find((option) => ["effort", "reasoningEffort", "thinkingLevel"].includes(option.id) && option.choices?.length);
  const effortValue = String((!thread.sessionId ? thread.config?.[effort?.id ?? ""] : undefined) ?? effort?.value ?? "");
  const effortLabel = effort?.choices?.find((item) => item.id === effortValue)?.label;
  const permission = thread.configurations?.find((option) => option.id === "executionProfile");
  const nativeValue = (id: string) => (!thread.sessionId ? thread.config?.[id] : undefined) ?? thread.configurations?.find((option) => option.id === id)?.value;
  const { label: permissionLabel, selected: permissionMode } = sessionPermissions(thread);
  const canConfigure = !loading && !thread.permissions.length && (!thread.sessionId || thread.capabilities?.configure?.availability === "available");
  const canEdit = (option?: { application: "creation_only" | "next_turn" | "current_turn" }) => canConfigure &&
    (!!option && (!thread.sessionId ? thread.state === "idle" : option.application !== "creation_only" && (thread.state === "idle" || option.application === "current_turn")));
  const fast = thread.configurations?.find((option) => option.id === "fast" && option.type === "boolean");
  const run = async (action: () => Promise<unknown>) => {
    setLoading(true);
    try {
      await action();
      return true;
    } catch (e) {
      store.setError(e);
      return false;
    } finally {
      setLoading(false);
    }
  };
  const fastControl = <Tooltip><Button variant="ghost" size="icon-sm" aria-label="Fast mode" aria-pressed={!!nativeValue("fast")}
    className={nativeValue("fast") ? "text-[#aaa8ef]" : "text-muted"} disabled={!canEdit(fast)}
    onClick={() => void run(() => store.configure(thread.id, "fast", !nativeValue("fast")))}><Zap size={16} /></Button>
    <TooltipContent>{!fast ? "Fast mode is not supported by this model" : nativeValue("fast") ? "Turn off Fast mode" : "Turn on Fast mode"}</TooltipContent></Tooltip>;
  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
      <Button
        variant="ghost"
        size="sm"
        className={settings ? `permissions-trigger rounded-full ${permissionMode === "full" ? "text-[#ff984e]" : ""}` : "model-trigger rounded-full"}
        title={settings ? "Change permissions" : undefined}
        aria-label={settings ? `Change permissions: ${permissionLabel}` : `Select model: ${modelLabel}${effortLabel ? `, ${effortLabel}` : ""}`}
        disabled={!connected || !thread.worktreeId}
      >
        {settings && <Shield size={14} />}<span className="model-trigger-label">{settings ? permissionLabel : <>{modelLabel}{effortLabel && ` · ${effortLabel}`}</>}</span>{!settings && <ChevronDown size={14} />}
      </Button>
      </PopoverTrigger>
      <PopoverContent side="top" align={settings ? "start" : "end"} className={settings ? "w-[440px]" : "w-72"} aria-label={settings ? "Permissions" : "Model and session settings"}>
        <div className="session-controls">
          <h2 className={settings ? "mb-1 text-sm text-muted" : "sr-only"}>{settings ? "How should actions be approved?" : "Model and reasoning effort"}</h2>
          {loading && <p className="muted" role="status">Reading session options…</p>}
          {!settings && !effort?.choices && models.length > 0 && <div className="flex items-center gap-2">{fastControl}<ModelPicker choices={models} value={model} label={modelLabel}
            disabled={!canEdit(modelOption)}
            onChange={(value) => run(() => store.configure(thread.id, "model", value))} /></div>}
          {!settings && effort?.choices && <ReasoningEffort key={`${model}:${effort.id}`} choices={effort.choices} value={effortValue}
            disabled={!canEdit(effort)} fastControl={fastControl}
            modelControl={(label) => <ModelPicker choices={models} value={model} label={modelLabel} effort={label}
              disabled={!canEdit(modelOption)}
              onChange={(value) => run(() => store.configure(thread.id, "model", value))} />}
            onChange={(value) => run(() => store.configure(thread.id, effort.id, value))} />}
          {previewError && <p className="text-xs text-muted" role="status">{previewError}</p>}
          {!settings && !loading && !previewError && !models.length && <p className="muted">This harness does not advertise a model catalog.</p>}
          {settings && !loading && !previewError && !permission && <p className="muted">This harness does not advertise permission settings.</p>}
          {thread.sessionId && thread.capabilities?.configure?.availability !== "available" && thread.capabilities?.configure?.reason && <p className="text-xs text-muted" role="status">{thread.capabilities.configure.reason}</p>}
          {settings && <PermissionPicker thread={thread} disabled={!canConfigure} onChange={(patch) => run(() => store.configureMany(thread.id, patch))} />}

        </div>
      </PopoverContent>
    </Popover>
  );
}
