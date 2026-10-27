import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, within } from "storybook/test";

import { ChromeButton } from "./chrome-button";
import { Icon } from "./icon";

const meta = { title: "Atoms/Chrome Button", component: ChromeButton } satisfies Meta<
  typeof ChromeButton
>;
export default meta;
type Story = StoryObj<typeof meta>;
export const Geometry: Story = {
  render: () => (
    <div className="flex flex-wrap items-center gap-4">
      <ChromeButton aria-label="Show navigation">
        <Icon name="sidebar" />
      </ChromeButton>
      <ChromeButton size="rail" selected aria-label="Conversations">
        <Icon name="chat" />
      </ChromeButton>
      <ChromeButton size="tabClose" aria-label="Close tab">
        <Icon name="close" />
      </ChromeButton>
      <ChromeButton size="navigationHeader">
        <Icon name="plus" />
        New conversation
      </ChromeButton>
      <ChromeButton size="composer" aria-label="Add attachment">
        <Icon name="plus" />
      </ChromeButton>
      <ChromeButton size="composerCap">
        Select model
        <Icon name="chevron" />
      </ChromeButton>
      <ChromeButton size="composerSend" aria-label="Send message">
        <Icon name="arrow" />
      </ChromeButton>
      <div role="tablist" aria-label="Views">
        <ChromeButton size="tab" selected>
          Conversation
        </ChromeButton>
      </div>
    </div>
  ),
  play: async ({ canvasElement }) => {
    const tab = within(canvasElement).getByRole("tab", { name: "Conversation" });
    await expect(tab).toHaveAttribute("aria-selected", "true");
    await expect(
      within(canvasElement).getByRole("button", { name: "Show navigation" }),
    ).toHaveAttribute("type", "button");
  },
};
export const Disabled: Story = {
  args: { isDisabled: true, "aria-label": "Unavailable action", children: <Icon name="plus" /> },
};
