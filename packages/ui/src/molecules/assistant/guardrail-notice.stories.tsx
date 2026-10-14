import type { Meta, StoryObj } from "@storybook/react-vite";

import { ExampleFrame } from "./examples/fixtures";
import { ReasoningMessageExample } from "./examples/reasoning-messages";

const meta = {
  title: "Assistant/Approvals & forms/GuardrailNotice",
  parameters: {
    layout: "fullscreen",
    docs: {
      description: {
        component:
          "Swarmie-owned guardrailnotice presentation contracts. All examples use synthetic data; applications supply execution and transport through callbacks.",
      },
    },
  },
} satisfies Meta;
export default meta;
type Story = StoryObj<typeof meta>;

export const GuardrailNotice: Story = {
  name: "Guardrail notice",
  render: () => (
    <ExampleFrame>
      <ReasoningMessageExample slug="guardrail-notice" />
    </ExampleFrame>
  ),
};
