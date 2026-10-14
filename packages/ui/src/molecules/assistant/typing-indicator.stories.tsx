import type { Meta, StoryObj } from "@storybook/react-vite";

import { ExampleFrame } from "./examples/fixtures";
import { ReasoningMessageExample } from "./examples/reasoning-messages";

const meta = {
  title: "Assistant/Loading/TypingIndicator",
  parameters: {
    layout: "fullscreen",
    docs: {
      description: {
        component:
          "Swarmie-owned typingindicator presentation contracts. All examples use synthetic data; applications supply execution and transport through callbacks.",
      },
    },
  },
} satisfies Meta;
export default meta;
type Story = StoryObj<typeof meta>;

export const TypingIndicator: Story = {
  name: "Typing indicator",
  render: () => (
    <ExampleFrame>
      <ReasoningMessageExample slug="typing-indicator" />
    </ExampleFrame>
  ),
};
