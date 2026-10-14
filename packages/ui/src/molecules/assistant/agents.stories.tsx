import type { Meta, StoryObj } from "@storybook/react-vite";

import { ExampleFrame } from "./examples/fixtures";
import { StructuredAgentExample } from "./examples/structured-agents";

const meta = {
  title: "Assistant/Agents & tasks/Coordination",
  parameters: {
    layout: "fullscreen",
    docs: {
      description: {
        component:
          "Swarmie-owned coordination presentation contracts. All examples use synthetic data; applications supply execution and transport through callbacks.",
      },
    },
  },
} satisfies Meta;
export default meta;
type Story = StoryObj<typeof meta>;

export const SubagentList: Story = {
  name: "Subagent list",
  render: () => (
    <ExampleFrame>
      <StructuredAgentExample slug="subagent-list" />
    </ExampleFrame>
  ),
};

export const AgentStatus: Story = {
  name: "Agent status",
  render: () => (
    <ExampleFrame>
      <StructuredAgentExample slug="agent-status" />
    </ExampleFrame>
  ),
};

export const AgentCard: Story = {
  name: "Agent card",
  render: () => (
    <ExampleFrame>
      <StructuredAgentExample slug="agent-card" />
    </ExampleFrame>
  ),
};

export const AgentHandoff: Story = {
  name: "Handoff",
  render: () => (
    <ExampleFrame>
      <StructuredAgentExample slug="agent-handoff" />
    </ExampleFrame>
  ),
};

export const BackgroundInbox: Story = {
  name: "Background runs",
  render: () => (
    <ExampleFrame>
      <StructuredAgentExample slug="background-inbox" />
    </ExampleFrame>
  ),
};

export const CheckpointHistory: Story = {
  name: "Checkpoints",
  render: () => (
    <ExampleFrame>
      <StructuredAgentExample slug="checkpoint-history" />
    </ExampleFrame>
  ),
};

export const ScheduleCard: Story = {
  name: "Schedule",
  render: () => (
    <ExampleFrame>
      <StructuredAgentExample slug="schedule-card" />
    </ExampleFrame>
  ),
};
