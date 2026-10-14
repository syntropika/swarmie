import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, userEvent, within } from "storybook/test";

import { ExampleFrame } from "./examples/fixtures";
import { ReasoningMessageExample } from "./examples/reasoning-messages";

const meta = {
  title: "Assistant/Messages/Message lifecycle",
  parameters: {
    layout: "fullscreen",
    docs: {
      description: {
        component:
          "Swarmie-owned React presentation contracts adapted from the Assistant UI messages concepts. All examples use synthetic data; applications supply execution and transport through callbacks.",
      },
    },
  },
} satisfies Meta;
export default meta;
type Story = StoryObj<typeof meta>;

export const MessagePair: Story = {
  name: "Message pair",
  render: () => (
    <ExampleFrame>
      <ReasoningMessageExample slug="message-pair" />
    </ExampleFrame>
  ),
};

export const MessageBranches: Story = {
  name: "Message branches",
  render: () => (
    <ExampleFrame>
      <ReasoningMessageExample slug="message-branches" />
    </ExampleFrame>
  ),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await userEvent.click(canvas.getByRole("button", { name: "Next response" }));
    await expect(canvas.getByText("2 / 3")).toBeVisible();
    await userEvent.click(canvas.getByRole("button", { name: "Previous response" }));
    await expect(canvas.getByText("1 / 3")).toBeVisible();
  },
};

export const ErrorState: Story = {
  name: "Error state",
  render: () => (
    <ExampleFrame>
      <ReasoningMessageExample slug="error-state" />
    </ExampleFrame>
  ),
};

export const MessageQueue: Story = {
  name: "Message queue",
  render: () => (
    <ExampleFrame>
      <ReasoningMessageExample slug="message-queue" />
    </ExampleFrame>
  ),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await userEvent.click(
      canvas.getByRole("button", { name: "Remove: Run the interaction checks." }),
    );
    await expect(canvas.queryByText("Run the interaction checks.")).not.toBeInTheDocument();
  },
};

export const EditMessage: Story = {
  name: "Edit a sent message",
  render: () => (
    <ExampleFrame>
      <ReasoningMessageExample slug="edit-message" />
    </ExampleFrame>
  ),
};

export const FeedbackDialog: Story = {
  name: "Feedback dialog",
  render: () => (
    <ExampleFrame>
      <ReasoningMessageExample slug="feedback-dialog" />
    </ExampleFrame>
  ),
};

export const StoppedRun: Story = {
  name: "Stopped run",
  render: () => (
    <ExampleFrame>
      <ReasoningMessageExample slug="stopped-run" />
    </ExampleFrame>
  ),
};

export const DaySeparator: Story = {
  name: "Timestamps",
  render: () => (
    <ExampleFrame>
      <ReasoningMessageExample slug="day-separator" />
    </ExampleFrame>
  ),
};

export const SpeakerIdentity: Story = {
  name: "Speaker identity",
  render: () => (
    <ExampleFrame>
      <ReasoningMessageExample slug="speaker-identity" />
    </ExampleFrame>
  ),
};

export const RegenerateMenu: Story = {
  name: "Regenerate with",
  render: () => (
    <ExampleFrame>
      <ReasoningMessageExample slug="regenerate-menu" />
    </ExampleFrame>
  ),
};

export const ConfidenceMarker: Story = {
  name: "Confidence",
  render: () => (
    <ExampleFrame>
      <ReasoningMessageExample slug="confidence-marker" />
    </ExampleFrame>
  ),
};
