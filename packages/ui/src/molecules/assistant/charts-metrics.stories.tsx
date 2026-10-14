import type { Meta, StoryObj } from "@storybook/react-vite";

import { ExampleFrame } from "./examples/fixtures";
import { StructuredAgentExample } from "./examples/structured-agents";

const meta = {
  title: "Assistant/Content & data/Charts & metrics",
  parameters: {
    layout: "fullscreen",
    docs: {
      description: {
        component:
          "Swarmie-owned charts & metrics presentation contracts. All examples use synthetic data; applications supply execution and transport through callbacks.",
      },
    },
  },
} satisfies Meta;
export default meta;
type Story = StoryObj<typeof meta>;

export const NumberTicker: Story = {
  name: "Number ticker",
  render: () => (
    <ExampleFrame>
      <StructuredAgentExample slug="number-ticker" />
    </ExampleFrame>
  ),
};

export const Chart: Story = {
  name: "Chart",
  render: () => (
    <ExampleFrame>
      <StructuredAgentExample slug="chart" />
    </ExampleFrame>
  ),
};

export const ScoreBreakdown: Story = {
  name: "Score breakdown",
  render: () => (
    <ExampleFrame>
      <StructuredAgentExample slug="score-breakdown" />
    </ExampleFrame>
  ),
};
