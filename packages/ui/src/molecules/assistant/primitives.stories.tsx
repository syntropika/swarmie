import type { Meta, StoryObj } from "@storybook/react-vite";

import { ConnectedRendererExample } from "./examples/connected-renderers";
import { ExampleFrame } from "./examples/fixtures";

const meta = {
  title: "Assistant/Primitives/Controls & icons",
  parameters: {
    layout: "fullscreen",
    docs: {
      description: {
        component:
          "Swarmie-owned controls & icons presentation contracts. All examples use synthetic data; applications supply execution and transport through callbacks.",
      },
    },
  },
} satisfies Meta;
export default meta;
type Story = StoryObj<typeof meta>;

export const TooltipIconButton: Story = {
  name: "Tooltip icon button",
  render: () => (
    <ExampleFrame>
      <ConnectedRendererExample slug="tooltip-icon-button" />
    </ExampleFrame>
  ),
};

export const Logos: Story = {
  name: "Model logos",
  render: () => (
    <ExampleFrame>
      <ConnectedRendererExample slug="logos" />
    </ExampleFrame>
  ),
};
