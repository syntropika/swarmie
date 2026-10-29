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
      <ChromeButton size="rail" aria-current="page" aria-label="Conversations">
        <Icon name="chat" />
      </ChromeButton>
      <ChromeButton size="tabClose" aria-label="Close tab">
        <Icon name="close" />
      </ChromeButton>
      <ChromeButton size="navigationHeader">
        <Icon name="plus" />
        New conversation
      </ChromeButton>
      <ChromeButton size="project">Project title</ChromeButton>
      <ChromeButton size="navigation">Conversation title</ChromeButton>
      <ChromeButton size="category">Settings category</ChromeButton>
      <ChromeButton size="action">Add folder</ChromeButton>
      <ChromeButton size="panelSwitch" selected>
        Conversation pane
      </ChromeButton>
      <ChromeButton size="composerAdd" aria-label="Add to message">
        <Icon name="plus" />
      </ChromeButton>
      <ChromeButton size="composerModel" aria-label="Example model, medium thinking">
        <Icon name="agent" />
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

export const InteractionStates: Story = {
  render: () => (
    <div className="grid gap-4">
      {(["light", "dark"] as const).map((theme) => (
        <div
          key={theme}
          data-theme={theme}
          className="flex flex-wrap items-center gap-4 bg-canvas p-4 text-ink"
        >
          <ChromeButton aria-label="Available action">
            <Icon name="plus" />
          </ChromeButton>
          <ChromeButton aria-expanded aria-label="Open action menu">
            <Icon name="dots" />
          </ChromeButton>
          <ChromeButton isDisabled aria-label="Unavailable action">
            <Icon name="plus" />
          </ChromeButton>
          <ChromeButton isPending isDisabled aria-label="Pending action">
            <Icon name="plus" />
          </ChromeButton>
          <ChromeButton size="rail" aria-current="page" aria-label="Current destination">
            <Icon name="chat" />
          </ChromeButton>
          <ChromeButton size="composerSend" aria-label="Send message">
            <Icon name="arrow-up" />
          </ChromeButton>
          <ChromeButton size="composerSend" aria-label="Stop response">
            <Icon name="stop" />
          </ChromeButton>
          <ChromeButton size="composerSend" isDisabled aria-label="Unavailable send">
            <Icon name="arrow-up" />
          </ChromeButton>
        </div>
      ))}
    </div>
  ),
};
