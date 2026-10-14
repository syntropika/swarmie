import type { Meta, StoryObj } from "@storybook/react-vite";

import { ConnectedRendererExample } from "./examples/connected-renderers";
import { ExampleFrame } from "./examples/fixtures";

const meta = {
  title: "Assistant/Content & data/HeatGraph",
  parameters: {
    layout: "fullscreen",
    docs: {
      description: {
        component:
          "Swarmie-owned heatgraph presentation contracts. All examples use synthetic data; applications supply execution and transport through callbacks.",
      },
    },
  },
} satisfies Meta;
export default meta;
type Story = StoryObj<typeof meta>;

export const HeatGraph: Story = {
  name: "Heat graph",
  render: () => (
    <ExampleFrame>
      <ConnectedRendererExample slug="heat-graph" />
    </ExampleFrame>
  ),
};
