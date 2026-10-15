import type { Meta, StoryObj } from "@storybook/react-vite";

import { ExampleFrame } from "./examples/fixtures";
import { ReasoningMessageExample } from "./examples/reasoning-messages";

const meta = {
  title: "Assistant/Loading/AssistantLoader",
  parameters: {
    layout: "fullscreen",
    docs: {
      description: {
        component:
          "Swarmie-owned assistantloader presentation contracts. All examples use synthetic data; applications supply execution and transport through callbacks.",
      },
    },
  },
} satisfies Meta;
export default meta;
type Story = StoryObj<typeof meta>;

export const LoadingState: Story = {
  name: "Loader",
  render: () => (
    <ExampleFrame>
      <ReasoningMessageExample slug="loading-state" />
    </ExampleFrame>
  ),
};
