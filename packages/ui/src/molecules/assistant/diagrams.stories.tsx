import type { Meta, StoryObj } from "@storybook/react-vite";

import { ExampleFrame } from "./examples/fixtures";
import { StructuredAgentExample } from "./examples/structured-agents";

const meta = {
  title: "Assistant/Content & data/Diagrams",
  parameters: {
    layout: "fullscreen",
    docs: {
      description: {
        component:
          "Swarmie-owned diagrams presentation contracts. All examples use synthetic data; applications supply execution and transport through callbacks.",
      },
    },
  },
} satisfies Meta;
export default meta;
type Story = StoryObj<typeof meta>;

export const Diagram: Story = {
  name: "Diagram",
  render: () => (
    <ExampleFrame>
      <StructuredAgentExample slug="diagram" />
    </ExampleFrame>
  ),
};

export const FlowGraph: Story = {
  name: "Flow graph",
  render: () => (
    <ExampleFrame>
      <StructuredAgentExample slug="flow-graph" />
    </ExampleFrame>
  ),
};

export const ActivityGraph: Story = {
  name: "Activity graph",
  render: () => (
    <ExampleFrame>
      <StructuredAgentExample slug="activity-graph" />
    </ExampleFrame>
  ),
};
