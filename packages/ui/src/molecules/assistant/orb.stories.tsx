import type { Meta, StoryObj } from "@storybook/react-vite";

import { ConnectedRendererExample } from "./examples/connected-renderers";
import { ExampleFrame } from "./examples/fixtures";

const meta = {
  title: "Assistant/Voice/Orb",
  parameters: {
    layout: "fullscreen",
    docs: {
      description: {
        component:
          "Swarmie-owned orb presentation contracts. All examples use synthetic data; applications supply execution and transport through callbacks.",
      },
    },
  },
} satisfies Meta;
export default meta;
type Story = StoryObj<typeof meta>;

export const Orb: Story = {
  name: "Orb",
  render: () => (
    <ExampleFrame>
      <ConnectedRendererExample slug="orb" />
    </ExampleFrame>
  ),
};
