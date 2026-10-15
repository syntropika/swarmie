import type { Meta, StoryObj } from "@storybook/react-vite";

import { ConnectedRendererExample } from "./examples/connected-renderers";
import { ExampleFrame } from "./examples/fixtures";

const meta = {
  title: "Assistant/Messages/Quote",
  parameters: {
    layout: "fullscreen",
    docs: {
      description: {
        component:
          "Swarmie-owned quote presentation contracts. All examples use synthetic data; applications supply execution and transport through callbacks.",
      },
    },
  },
} satisfies Meta;
export default meta;
type Story = StoryObj<typeof meta>;

export const Quote: Story = {
  name: "Quote",
  render: () => (
    <ExampleFrame>
      <ConnectedRendererExample slug="quote" />
    </ExampleFrame>
  ),
};
