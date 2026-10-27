import type { Draft } from "@swarmie/plugin-conversations-browser";
import type { ProviderModel } from "@swarmie/plugin-providers-browser";
import { useState } from "react";

import { Item, Menu } from "../../controls";

export function ComposerModel({
  draft,
  models,
  onSelectModel,
  onSelectEffort,
}: {
  draft: Draft;
  models: readonly ProviderModel[];
  onSelectModel: (id: string) => void;
  onSelectEffort: (effort: string) => void;
}) {
  const [effortOpen, setEffortOpen] = useState(false);
  const selectedModel = models.find((model) => model.id === draft.model);
  return (
    <Menu
      label="Select model and effort"
      text={selectedModel?.label ?? "Select model"}
      size="composerCap"
      placement="top end"
    >
      {(close) => (
        <>
          {models.map((model) => (
            <Item
              key={model.id}
              detail={model.id === draft.model ? "✓" : ""}
              onClick={() => {
                onSelectModel(model.id);
                close();
              }}
            >
              <span className="block">{model.label}</span>
              {model.description === undefined ? null : (
                <span className="block text-xs text-muted">{model.description}</span>
              )}
            </Item>
          ))}
          {models.length === 0 ? (
            <p className="px-2 py-3 text-xs text-muted">
              No models are available for this account.
            </p>
          ) : null}
          {selectedModel !== undefined && selectedModel.efforts.length > 0 ? (
            <>
              <div className="my-1 border-t border-border" />
              <Item
                detail={draft.effort}
                onClick={() => {
                  setEffortOpen((value) => !value);
                }}
              >
                Effort
              </Item>
              {effortOpen
                ? selectedModel.efforts.map((effort) => (
                    <Item
                      key={effort}
                      detail={effort === draft.effort ? "✓" : ""}
                      onClick={() => {
                        onSelectEffort(effort);
                      }}
                    >
                      {effort}
                    </Item>
                  ))
                : null}
            </>
          ) : null}
        </>
      )}
    </Menu>
  );
}
