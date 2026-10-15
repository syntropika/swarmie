import type { Meta, StoryObj } from "@storybook/react-vite";

import { ConnectedRendererExample } from "./examples/connected-renderers";
import { ExampleFrame } from "./examples/fixtures";

const meta = {
  title: "Assistant/Messages/Follow-up suggestions",
  parameters: {
    layout: "fullscreen",
    docs: {
      description: {
        component:
          "Swarmie-owned follow-up suggestions presentation contracts. All examples use synthetic data; applications supply execution and transport through callbacks.",
      },
    },
  },
} satisfies Meta;
export default meta;
type Story = StoryObj<typeof meta>;

export const FollowUpSuggestions: Story = {
  name: "Follow-up suggestions",
  render: () => (
    <ExampleFrame>
      <ConnectedRendererExample slug="follow-up-suggestions" />
    </ExampleFrame>
  ),
};
