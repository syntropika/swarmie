import type { Meta, StoryObj } from "@storybook/react-vite";

import { ConnectedRendererExample } from "./examples/connected-renderers";
import { ExampleFrame } from "./examples/fixtures";

const meta = {
  title: "Assistant/Generative UI/Renderer",
  parameters: {
    layout: "fullscreen",
    docs: {
      description: {
        component:
          "Swarmie-owned renderer presentation contracts. All examples use synthetic data; applications supply execution and transport through callbacks.",
      },
    },
  },
} satisfies Meta;
export default meta;
type Story = StoryObj<typeof meta>;

export const GenerativeUi: Story = {
  name: "Generative UI",
  render: () => (
    <ExampleFrame>
      <ConnectedRendererExample slug="generative-ui" />
    </ExampleFrame>
  ),
};
