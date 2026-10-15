import type { Meta, StoryObj } from "@storybook/react-vite";

import { ExampleFrame } from "./examples/fixtures";
import { StructuredAgentExample } from "./examples/structured-agents";

const meta = {
  title: "Assistant/Content & data/SpecSheet",
  parameters: {
    layout: "fullscreen",
    docs: {
      description: {
        component:
          "Swarmie-owned specsheet presentation contracts. All examples use synthetic data; applications supply execution and transport through callbacks.",
      },
    },
  },
} satisfies Meta;
export default meta;
type Story = StoryObj<typeof meta>;

export const SpecSheet: Story = {
  name: "Spec sheet",
  render: () => (
    <ExampleFrame>
      <StructuredAgentExample slug="spec-sheet" />
    </ExampleFrame>
  ),
};
