import type { Meta, StoryObj } from "@storybook/react-vite";

import { ExampleFrame } from "./examples/fixtures";
import { ThinkingState } from "./thinking-state";

const meta = {
  title: "Assistant/Loading/ThinkingState",
  component: ThinkingState,
  parameters: { layout: "fullscreen" },
} satisfies Meta<typeof ThinkingState>;
export default meta;
type Story = StoryObj;

export const Default: Story = {
  render: () => (
    <ExampleFrame>
      <output>
        <ThinkingState>Thinking</ThinkingState>
      </output>
    </ExampleFrame>
  ),
};
