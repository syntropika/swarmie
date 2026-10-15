import type { Meta, StoryObj } from "@storybook/react-vite";

import { ExampleFrame } from "./examples/fixtures";
import { StructuredAgentExample } from "./examples/structured-agents";

const meta = {
  title: "Assistant/Agents & tasks/Plans & tasks",
  parameters: {
    layout: "fullscreen",
    docs: {
      description: {
        component:
          "Swarmie-owned plans & tasks presentation contracts. All examples use synthetic data; applications supply execution and transport through callbacks.",
      },
    },
  },
} satisfies Meta;
export default meta;
type Story = StoryObj<typeof meta>;

export const AgentPlan: Story = {
  name: "Agent plan",
  render: () => (
    <ExampleFrame>
      <StructuredAgentExample slug="agent-plan" />
    </ExampleFrame>
  ),
};

export const TaskCard: Story = {
  name: "Task card",
  render: () => (
    <ExampleFrame>
      <StructuredAgentExample slug="task-card" />
    </ExampleFrame>
  ),
};

export const TodoList: Story = {
  name: "Todo list",
  render: () => (
    <ExampleFrame>
      <StructuredAgentExample slug="todo-list" />
    </ExampleFrame>
  ),
};
