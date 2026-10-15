import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, userEvent, within } from "storybook/test";

import { ComposerThreadExample } from "./examples/composer-thread";
import { ExampleFrame } from "./examples/fixtures";

const meta = {
  title: "Assistant/Conversations/Thread",
  parameters: {
    layout: "fullscreen",
    docs: {
      description: {
        component:
          "Swarmie-owned thread presentation contracts. All examples use synthetic data; applications supply execution and transport through callbacks.",
      },
    },
  },
} satisfies Meta;
export default meta;
type Story = StoryObj<typeof meta>;

export const ChatPanel: Story = {
  name: "Chat panel",
  render: () => (
    <ExampleFrame>
      <ComposerThreadExample slug="chat-panel" />
    </ExampleFrame>
  ),
};

export const EmptyState: Story = {
  name: "Empty state",
  render: () => (
    <ExampleFrame>
      <ComposerThreadExample slug="empty-state" />
    </ExampleFrame>
  ),
};

export const ScrollAnchor: Story = {
  name: "Scroll anchor",
  render: () => (
    <ExampleFrame>
      <ComposerThreadExample slug="scroll-anchor" />
    </ExampleFrame>
  ),
};

export const CanvasSplit: Story = {
  name: "Canvas",
  render: () => (
    <ExampleFrame>
      <ComposerThreadExample slug="canvas-split" />
    </ExampleFrame>
  ),
};

export const ConnectionState: Story = {
  name: "Connection state",
  render: () => (
    <ExampleFrame>
      <ComposerThreadExample slug="connection-state" />
    </ExampleFrame>
  ),
};

export const SharedConversation: Story = {
  name: "Shared conversation",
  render: () => (
    <ExampleFrame>
      <ComposerThreadExample slug="shared-conversation" />
    </ExampleFrame>
  ),
};

export const ConversationSearch: Story = {
  name: "Search in conversation",
  render: () => (
    <ExampleFrame>
      <ComposerThreadExample slug="conversation-search" />
    </ExampleFrame>
  ),
};

export const ThreadSearch: Story = {
  name: "Thread search",
  render: () => (
    <ExampleFrame>
      <ComposerThreadExample slug="thread-search" />
    </ExampleFrame>
  ),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await userEvent.type(canvas.getByRole("searchbox", { name: "Search sessions" }), "invitation");
    await expect(canvas.getByRole("button", { name: /Review the invitation flow/u })).toBeVisible();
    await expect(
      canvas.queryByRole("button", { name: /Assistant interface design/u }),
    ).not.toBeInTheDocument();
  },
};

export const LauncherBubble: Story = {
  name: "Launcher",
  render: () => (
    <ExampleFrame>
      <ComposerThreadExample slug="launcher-bubble" />
    </ExampleFrame>
  ),
};

export const SettingsPanel: Story = {
  name: "Settings",
  render: () => (
    <ExampleFrame>
      <ComposerThreadExample slug="settings-panel" />
    </ExampleFrame>
  ),
};

export const Onboarding: Story = {
  name: "Onboarding",
  render: () => (
    <ExampleFrame>
      <ComposerThreadExample slug="onboarding" />
    </ExampleFrame>
  ),
};
