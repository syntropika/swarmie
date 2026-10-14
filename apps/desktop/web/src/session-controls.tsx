import { useQuery } from "@tanstack/react-query";
import { ChevronDown, Shield, Zap } from "lucide-react";
import { useState } from "react";

import { Button } from "@/components/ui/button";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { Tooltip, TooltipContent } from "@/components/ui/tooltip";

import { startAction, errorMessage } from "./action-boundary";
import { scalarLabel } from "./components/presentation-values";
import { ModelPicker } from "./model-picker";
import { nodeKey } from "./node-scope";
import { PermissionPicker } from "./permission-picker";
import type { Thread, Configuration } from "./protocol";
import { ReasoningEffort } from "./reasoning-effort";
import { sessionPermissions } from "./session-permissions";
import { hasText, hasNumber, nonEmptyText } from "./value-presence";
import type { WorkspaceStore } from "./workspace";

type ControlProps = {
  thread: Thread;
  store: WorkspaceStore;
  connected: boolean;
  kind?: "model" | "settings";
};
function nativeValue(thread: Thread, id: string) {
  return (
    (hasText(thread.sessionId) ? undefined : thread.config?.[id]) ??
    thread.configurations?.find((option) => option.id === id)?.value
  );
}
function modelOptions(thread: Thread) {
  const modelOption = thread.configurations?.find((option) => option.id === "model");
  const models = hasNumber(modelOption?.choices?.length)
    ? modelOption.choices
    : (thread.models ?? []);
  const model = nonEmptyText(thread.model) ?? scalarLabel(modelOption?.value, "");
  const modelLabel =
    nonEmptyText(models.find((item) => item.id === model)?.label) ??
    nonEmptyText(model) ??
    nonEmptyText(thread.actualModel) ??
    "Select model";
  return { modelOption, models, model, modelLabel };
}
function effortOptions(thread: Thread) {
  const effort = thread.configurations?.find(
    (option) =>
      ["effort", "reasoningEffort", "thinkingLevel"].includes(option.id) &&
      hasNumber(option.choices?.length),
  );
  const effortValue = scalarLabel(nativeValue(thread, effort?.id ?? ""), "");
  const effortLabel = effort?.choices?.find((item) => item.id === effortValue)?.label;
  return { effort, effortValue, effortLabel };
}
function canEdit(thread: Thread, canConfigure: boolean, option?: Configuration) {
  if (!canConfigure || option === undefined) {
    return false;
  }
  if (!hasText(thread.sessionId)) {
    return thread.state === "idle";
  }
  return (
    option.application !== "creation_only" &&
    (thread.state === "idle" || option.application === "current_turn")
  );
}
function useSessionControls({ thread, store, connected }: ControlProps) {
  const [working, setWorking] = useState(false);
  const { node } = store.getSnapshot();
  const query = useQuery({
    queryKey: [
      "session-options",
      node ? nodeKey(node) : "",
      thread.id,
      thread.harness,
      thread.worktreeId,
      thread.model,
      thread.sessionId,
      thread.state,
      thread.config ?? {},
    ],
    queryFn: async () => {
      try {
        await store.previewControls(thread.id);
      } catch (error) {
        if (errorMessage(error).toLowerCase() !== "session is busy") {
          throw error;
        }
      }
      return null;
    },
    enabled: connected && hasText(thread.worktreeId) && thread.state === "idle",
  });
  const loading = working || query.isFetching;
  const previewError = query.isError ? errorMessage(query.error) : "";
  const run = async (action: () => Promise<unknown>) => {
    setWorking(true);
    try {
      await action();
      return true;
    } catch (error) {
      store.setError(error);
      return false;
    } finally {
      setWorking(false);
    }
  };
  const canConfigure =
    !loading &&
    thread.permissions.length === 0 &&
    (!hasText(thread.sessionId) || thread.capabilities?.configure?.availability === "available");
  return {
    ...modelOptions(thread),
    ...effortOptions(thread),
    loading,
    previewError,
    run,
    canConfigure,
  };
}
type Controls = ReturnType<typeof useSessionControls>;
export function SessionControls(props: ControlProps) {
  const { thread, connected, kind = "model" } = props;
  const settings = kind === "settings";
  const [open, setOpen] = useState(false);
  const controls = useSessionControls(props);
  const permission = sessionPermissions(thread);
  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <Button
          variant="composer"
          shape="pill"
          size="sm"
          title={settings ? "Change permissions" : undefined}
          aria-label={
            settings
              ? `Change permissions: ${permission.label}`
              : `Select model: ${controls.modelLabel}${hasText(controls.effortLabel) ? `, ${controls.effortLabel}` : ""}`
          }
          disabled={!connected || !hasText(thread.worktreeId)}
        >
          <span
            className={`flex items-center gap-2 ${settings && permission.selected === "full" ? "text-permission-full-access" : ""}`}
          >
            {settings && <Shield size={14} />}
            <span className="model-trigger-label">
              {settings ? permission.label : controls.modelLabel}
              {!settings && hasText(controls.effortLabel) && ` · ${controls.effortLabel}`}
            </span>
            {!settings && <ChevronDown size={14} />}
          </span>
        </Button>
      </PopoverTrigger>
      <PopoverContent
        side="top"
        align={settings ? "start" : "end"}
        className={settings ? "w-[440px]" : "w-72"}
        aria-label={settings ? "Permissions" : "Model and session settings"}
      >
        <div className="session-controls">
          <h2 className={settings ? "text-muted mb-1 text-sm" : "sr-only"}>
            {settings ? "How should actions be approved?" : "Model and reasoning effort"}
          </h2>
          <SessionControlStatus thread={thread} controls={controls} settings={settings} />
          {settings ? (
            <PermissionPicker
              thread={thread}
              disabled={!controls.canConfigure}
              onChange={async (patch) =>
                await controls.run(async () => {
                  await props.store.configureMany(thread.id, patch);
                })
              }
            />
          ) : (
            <ModelSettings {...props} controls={controls} />
          )}
        </div>
      </PopoverContent>
    </Popover>
  );
}
function SessionControlStatus({
  thread,
  controls,
  settings,
}: {
  thread: Thread;
  controls: Controls;
  settings: boolean;
}) {
  const { loading, previewError, models } = controls;
  const permission = thread.configurations?.find((option) => option.id === "executionProfile");
  return (
    <>
      {loading && <output className="muted">Reading session options…</output>}
      {hasText(previewError) && <output className="text-muted text-xs">{previewError}</output>}
      {!settings && !loading && !hasText(previewError) && models.length === 0 && (
        <p className="muted">This agent tool does not advertise a model catalog.</p>
      )}
      {settings && !loading && !hasText(previewError) && permission === undefined && (
        <p className="muted">This agent tool does not advertise permission settings.</p>
      )}
      {hasText(thread.sessionId) &&
        thread.capabilities?.configure?.availability !== "available" &&
        hasText(thread.capabilities?.configure?.reason) && (
          <output className="text-muted text-xs">{thread.capabilities.configure.reason}</output>
        )}
    </>
  );
}
function ModelSettings({ thread, store, controls }: ControlProps & { controls: Controls }) {
  const { models, model, modelLabel, modelOption, effort, effortValue, run, canConfigure } =
    controls;
  const chooseModel = async (value: string) =>
    await run(async () => {
      await store.configure(thread.id, "model", value);
    });
  const fastControl = <FastControl thread={thread} store={store} controls={controls} />;
  if (effort?.choices !== undefined) {
    return (
      <ReasoningEffort
        key={`${model}:${effort.id}`}
        choices={effort.choices}
        value={effortValue}
        disabled={!canEdit(thread, canConfigure, effort)}
        fastControl={fastControl}
        renderModel={(label) => (
          <ModelPicker
            choices={models}
            value={model}
            label={modelLabel}
            effort={label}
            disabled={!canEdit(thread, canConfigure, modelOption)}
            onChange={chooseModel}
          />
        )}
        onChange={async (value) =>
          await run(async () => {
            await store.configure(thread.id, effort.id, value);
          })
        }
      />
    );
  }
  if (models.length === 0) {
    return null;
  }
  return (
    <div className="flex items-center gap-2">
      {fastControl}
      <ModelPicker
        choices={models}
        value={model}
        label={modelLabel}
        disabled={!canEdit(thread, canConfigure, modelOption)}
        onChange={chooseModel}
      />
    </div>
  );
}
function FastControl({
  thread,
  store,
  controls,
}: Pick<ControlProps, "thread" | "store"> & { controls: Controls }) {
  const fast = thread.configurations?.find(
    (option) => option.id === "fast" && option.type === "boolean",
  );
  const enabled = nativeValue(thread, "fast") === true;
  let label = "Fast mode is not supported by this model";
  if (fast !== undefined) {
    label = enabled ? "Turn off Fast mode" : "Turn on Fast mode";
  }
  return (
    <Tooltip>
      <Button
        variant="ghost"
        size="icon-sm"
        aria-label="Fast mode"
        aria-pressed={enabled}
        disabled={!canEdit(thread, controls.canConfigure, fast)}
        onClick={() => {
          startAction(
            controls.run(async () => {
              await store.configure(thread.id, "fast", !enabled);
            }),
            store.setError.bind(store),
          );
        }}
      >
        <Zap size={16} className={enabled ? "text-focus" : "text-muted"} />
      </Button>
      <TooltipContent>{label}</TooltipContent>
    </Tooltip>
  );
}
