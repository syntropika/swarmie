import type { Meta, StoryObj } from "@storybook/react-vite";

import { ExampleFrame } from "./examples/fixtures";
import { ReasoningMessageExample } from "./examples/reasoning-messages";

const meta = {
  title: "Assistant/Content & data/StreamingText",
  parameters: {
    layout: "fullscreen",
    docs: {
      description: {
        component:
          "Swarmie-owned streamingtext presentation contracts. All examples use synthetic data; applications supply execution and transport through callbacks.",
      },
    },
  },
} satisfies Meta;
export default meta;
type Story = StoryObj<typeof meta>;

export const StreamingText: Story = {
  name: "Streaming text",
  render: () => (
    <ExampleFrame>
      <ReasoningMessageExample slug="streaming-text" />
    </ExampleFrame>
  ),
};
