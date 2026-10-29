import type { Meta, StoryObj } from "@storybook/react-vite";

import { ChromeButton } from "../atoms/chrome-button";
import { Icon } from "../atoms/icon";
import { ComposerPanel } from "./composer-panel";
import { ComposerTextArea } from "./composer-textarea";

const meta = {
  title: "Elements/Composer Panel",
  component: ComposerPanel,
  parameters: {
    docs: {
      description: {
        component: "Presentation slots for supplied execution context, draft and composer actions.",
      },
    },
  },
} satisfies Meta<typeof ComposerPanel>;
export default meta;
type Story = StoryObj<typeof meta>;

export const Conversation: Story = {
  render: () => (
    <div className="w-full max-w-[720px]">
      <ComposerPanel>
        <ComposerPanel.Context>
          <Icon name="agent" className="size-3.5" />
          No model selected
        </ComposerPanel.Context>
        <ComposerPanel.Body>
          <ComposerTextArea rows={2} aria-label="Message" placeholder="Write a message" />
          <div className="flex items-center justify-between gap-2">
            <ChromeButton size="composer" aria-label="Add to message">
              <Icon name="plus" />
            </ChromeButton>
            <div className="flex items-center gap-1">
              <ChromeButton size="composerCap">
                Select model
                <Icon name="chevron" />
              </ChromeButton>
              <ChromeButton size="composerSend" isDisabled aria-label="Send message">
                <Icon name="arrow" />
              </ChromeButton>
            </div>
          </div>
        </ComposerPanel.Body>
      </ComposerPanel>
    </div>
  ),
};
