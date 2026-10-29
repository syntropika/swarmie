import { Button, ListBox, Select } from "@swarmie/ui-web";
import { useEffect, useRef, useState } from "react";

import type { ModelDefaults, ModelDefaultsApi, ProviderAccount, ProviderModel } from "./index";
import { automaticModelDefaults } from "./index";
import { ReasoningMark } from "./reasoning-mark";
import { accountDefaultSelection, effortFor } from "./selection";

function ModelSelect({
  label,
  value,
  models,
  disabled,
  onChange,
}: {
  label: string;
  value: string | null;
  models: readonly ProviderModel[];
  disabled: boolean;
  onChange: (value: string | null) => void;
}) {
  const automatic = "automatic";
  return (
    <Select
      aria-label={label}
      value={value === null ? automatic : `model:${value}`}
      isDisabled={disabled}
      onChange={(key) => {
        if (key === automatic) {
          onChange(null);
        } else if (typeof key === "string" && key.startsWith("model:")) {
          onChange(key.slice(6));
        }
      }}
    >
      <Select.Trigger className="w-full min-w-0">
        <Select.Value />
        <Select.Indicator />
      </Select.Trigger>
      <Select.Popover>
        <ListBox>
          <ListBox.Item id={automatic} textValue="Automatic">
            Automatic
            <ListBox.ItemIndicator />
          </ListBox.Item>
          {models.map((model) => (
            <ListBox.Item key={model.id} id={`model:${model.id}`} textValue={model.label}>
              {model.label}
              <ListBox.ItemIndicator />
            </ListBox.Item>
          ))}
        </ListBox>
      </Select.Popover>
    </Select>
  );
}

export function ModelDefaultsSettings({
  account,
  api,
}: {
  account: ProviderAccount;
  api: ModelDefaultsApi | null;
}) {
  const [pending, setPending] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);
  const active = useRef<AbortController | null>(null);
  useEffect(
    () => () => {
      active.current?.abort();
    },
    [],
  );
  const defaults = account.defaults ?? automaticModelDefaults;
  const selection = accountDefaultSelection(account);
  const model = account.models.find((entry) => entry.id === selection?.model);
  const effort = selection?.effort ?? "";
  const disabled = api === null || pending || !account.available || account.models.length === 0;
  function save(next: ModelDefaults) {
    if (api === null || active.current !== null) {
      return;
    }
    const { save: saveDefaults } = api;
    const controller = new AbortController();
    active.current = controller;
    setPending(true);
    setSaved(false);
    setSaveError(null);
    function failed(error: unknown) {
      if (!controller.signal.aborted) {
        setSaveError(error instanceof Error ? error.message : "Model defaults could not be saved.");
      }
    }
    async function persist() {
      try {
        await saveDefaults(account.id, next, controller.signal);
        if (!controller.signal.aborted) {
          setSaved(true);
        }
      } catch (error: unknown) {
        failed(error);
      } finally {
        if (!controller.signal.aborted) {
          active.current = null;
          setPending(false);
        }
      }
    }
    persist().catch(failed);
  }
  const row =
    "grid min-w-0 items-center gap-3 py-4 @min-[600px]:grid-cols-[minmax(0,1fr)_minmax(220px,0.9fr)] @min-[600px]:gap-6";
  let status = "Defaults do not change existing conversations.";
  if (api === null) {
    status = "Connect a node to save model defaults.";
  }
  if (saved) {
    status = "Saved on this node.";
  }
  if (pending) {
    status = "Saving…";
  }
  return (
    <section aria-label="Model defaults" className="grid gap-3">
      <h2 className="text-sm font-medium">Model defaults</h2>
      <div className="divide-y divide-border rounded-(--radius-group) bg-group px-4">
        <div className={row}>
          <div className="grid gap-1">
            <h3 className="text-sm font-medium">Default model</h3>
            <p className="text-xs text-muted">Used for new conversations with this account.</p>
          </div>
          <ModelSelect
            label="Default model"
            models={account.models}
            value={defaults.model}
            disabled={disabled}
            onChange={(id) => {
              const next =
                id === null ? account.models[0] : account.models.find((entry) => entry.id === id);
              save({
                ...defaults,
                model: id,
                effort: next ? effortFor(next, effort) || null : null,
              });
            }}
          />
        </div>
        <div className={row}>
          <div className="grid gap-1">
            <h3 className="text-sm font-medium">Default reasoning</h3>
            <p className="text-xs text-muted">Uses the levels supported by the selected model.</p>
          </div>
          {model && model.efforts.length > 0 ? (
            <Select
              aria-label="Default reasoning"
              value={effort}
              isDisabled={disabled || model.efforts.length < 2}
              onChange={(value) => {
                if (typeof value === "string" && model.efforts.includes(value)) {
                  save({ ...defaults, effort: value });
                }
              }}
            >
              <Select.Trigger className="w-full min-w-0">
                <Select.Value />
                <Select.Indicator />
              </Select.Trigger>
              <Select.Popover>
                <ListBox>
                  {model.efforts.map((value) => (
                    <ListBox.Item key={value} id={value} textValue={value}>
                      <span className="flex min-w-0 items-center gap-2">
                        <ReasoningMark
                          providerId={account.providerId}
                          levels={model.efforts}
                          value={value}
                        />
                        <span>{value}</span>
                      </span>
                      <ListBox.ItemIndicator />
                    </ListBox.Item>
                  ))}
                </ListBox>
              </Select.Popover>
            </Select>
          ) : (
            <p className="text-sm text-muted">This model has no configurable reasoning levels.</p>
          )}
        </div>
        <div className={row}>
          <div className="grid gap-1">
            <h3 className="text-sm font-medium">Auxiliary model</h3>
            <p className="text-xs text-muted">
              For titles, progress descriptions and session summaries.
            </p>
          </div>
          <div className="grid min-w-0 gap-2">
            <ModelSelect
              label="Auxiliary model"
              models={account.models}
              value={defaults.auxiliaryModel}
              disabled={disabled}
              onChange={(id) => {
                save({ ...defaults, auxiliaryModel: id });
              }}
            />
            <p className="text-xs text-muted">
              Automatic uses the default model. This catalog does not publish a small-model
              recommendation.
            </p>
          </div>
        </div>
      </div>
      <div aria-live="polite" className="text-xs text-muted">
        {status}
      </div>
      {saveError === null ? null : (
        <div className="grid justify-items-start gap-2">
          <p role="alert" className="text-sm text-danger">
            {saveError}
          </p>
          <Button
            size="sm"
            variant="secondary"
            onPress={() => {
              setSaveError(null);
            }}
          >
            Dismiss
          </Button>
        </div>
      )}
    </section>
  );
}
