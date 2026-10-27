import type { Meta, StoryObj } from "@storybook/react-vite";
import { useState } from "react";

import { ChromeButton } from "../atoms/chrome-button";
import { Icon } from "../atoms/icon";
import type { IconName } from "../atoms/icon";
import { MessageActions } from "./message-actions";

const meta = { title: "Elements/Message Action Rows", component: MessageActions } satisfies Meta<
  typeof MessageActions
>;
export default meta;
type Story = StoryObj<typeof meta>;

function ActionRow({
  assistant,
  extended = false,
}: {
  readonly assistant: boolean;
  readonly extended?: boolean;
}) {
  const [lastAction, setLastAction] = useState("Ready");
  const actions: readonly { readonly label: string; readonly icon: IconName }[] = assistant
    ? [
        { label: "Copy response", icon: "copy" },
        { label: "Read aloud", icon: "volume" },
        { label: "Share response", icon: "share" },
        { label: "Regenerate response", icon: "refresh" },
        { label: "More response actions", icon: "dots" },
      ]
    : [
        { label: "Copy message", icon: "copy" },
        { label: "Share message", icon: "share" },
        { label: "Edit message", icon: "rename" },
      ];
  return (
    <div className="grid gap-3">
      <p className="text-caption">
        {assistant ? "The preview is ready for review." : "Please review this project."}
      </p>
      <MessageActions
        role="toolbar"
        aria-label={assistant ? "Assistant response actions" : "User message actions"}
      >
        {actions.map((action) => (
          <ChromeButton
            key={action.label}
            size="composer"
            aria-label={action.label}
            title={action.label}
            onPress={() => {
              setLastAction(action.label);
            }}
          >
            <Icon name={action.icon} />
          </ChromeButton>
        ))}
        {extended ? (
          <ChromeButton
            size="composer"
            aria-label="Plugin action"
            title="Plugin action"
            onPress={() => {
              setLastAction("Plugin action");
            }}
          >
            <Icon name="flag" />
          </ChromeButton>
        ) : null}
      </MessageActions>
      <output className="text-chrome text-muted">{lastAction}</output>
    </div>
  );
}
export const Assistant: Story = { render: () => <ActionRow assistant /> };
export const User: Story = { render: () => <ActionRow assistant={false} /> };
export const AdditionalPluginControl: Story = { render: () => <ActionRow assistant extended /> };
