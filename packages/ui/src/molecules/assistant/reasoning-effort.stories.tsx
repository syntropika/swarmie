import type { Meta, StoryObj } from "@storybook/react-vite";

import { ExampleFrame } from "./examples/fixtures";
import { ReasoningMessageExample } from "./examples/reasoning-messages";

const meta = {
  title: "Assistant/Reasoning/reasoning-effort",
  parameters: {
    layout: "fullscreen",
    docs: {
      description: {
        component:
          "Swarmie-owned reasoningeffort presentation contracts. All examples use synthetic data; applications supply execution and transport through callbacks.",
      },
    },
  },
} satisfies Meta;
export default meta;
type Story = StoryObj<typeof meta>;

export const ReasoningEffort: Story = {
  name: "Reasoning effort",
  render: () => (
    <ExampleFrame>
      <ReasoningMessageExample slug="reasoning-effort" />
    </ExampleFrame>
  ),
};
