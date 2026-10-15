import type { Meta, StoryObj } from "@storybook/react-vite";

import { ConnectedRendererExample } from "./examples/connected-renderers";
import { ExampleFrame } from "./examples/fixtures";

const meta = {
  title: "Assistant/Reasoning/Controlled reasoning",
  parameters: {
    layout: "fullscreen",
    docs: {
      description: {
        component:
          "Swarmie-owned controlled reasoning presentation contracts. All examples use synthetic data; applications supply execution and transport through callbacks.",
      },
    },
  },
} satisfies Meta;
export default meta;
type Story = StoryObj<typeof meta>;

export const Reasoning: Story = {
  name: "Reasoning",
  render: () => (
    <ExampleFrame>
      <ConnectedRendererExample slug="reasoning" />
    </ExampleFrame>
  ),
};
