import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, fireEvent, userEvent, waitFor, within } from "storybook/test";

import { ConnectedRendererExample } from "./examples/connected-renderers";
import { ExampleFrame } from "./examples/fixtures";

const meta = {
  title: "Assistant/Conversations/Controlled layouts & navigation",
  parameters: {
    layout: "fullscreen",
    docs: {
      description: {
        component:
          "Swarmie-owned controlled layouts & navigation presentation contracts. All examples use synthetic data; applications supply execution and transport through callbacks.",
      },
    },
  },
} satisfies Meta;
export default meta;
type Story = StoryObj<typeof meta>;

export const Thread: Story = {
  name: "Thread",
  render: () => (
    <ExampleFrame>
      <ConnectedRendererExample slug="thread" />
    </ExampleFrame>
  ),
};

export const AssistantModal: Story = {
  name: "Assistant modal",
  render: () => (
    <ExampleFrame>
      <ConnectedRendererExample slug="assistant-modal" />
    </ExampleFrame>
  ),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await userEvent.click(canvas.getByRole("button", { name: "Open assistant" }));
    const dialog = canvas.getByRole("dialog", { name: "Swarmie assistant" });
    await waitFor(async () => {
      await expect(dialog).toBeVisible();
    });
    // Synthetic keyboard events cannot invoke the browser's native dialog default action.
    // Exercise the cancel contract here; the browser check also sends a trusted Escape key.
    await fireEvent(dialog, new Event("cancel", { cancelable: true }));
    await waitFor(async () => {
      await expect(dialog).not.toBeVisible();
    });
    await expect(canvas.getByRole("button", { name: "Open assistant" })).toHaveFocus();
  },
};

export const AssistantSidebar: Story = {
  name: "Assistant sidebar",
  render: () => (
    <ExampleFrame>
      <ConnectedRendererExample slug="assistant-sidebar" />
    </ExampleFrame>
  ),
};

export const ThreadList: Story = {
  name: "Thread list",
  render: () => (
    <ExampleFrame>
      <ConnectedRendererExample slug="thread-list" />
    </ExampleFrame>
  ),
};

export const ThreadListSidebar: Story = {
  name: "Thread list sidebar",
  render: () => (
    <ExampleFrame>
      <ConnectedRendererExample slug="thread-list-sidebar" />
    </ExampleFrame>
  ),
};

export const ConversationMap: Story = {
  name: "Conversation map",
  render: () => (
    <ExampleFrame>
      <ConnectedRendererExample slug="conversation-map" />
    </ExampleFrame>
  ),
};
