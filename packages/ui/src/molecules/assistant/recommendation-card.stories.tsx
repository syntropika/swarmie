import type { Meta, StoryObj } from "@storybook/react-vite";

import { ExampleFrame } from "./examples/fixtures";
import { StructuredAgentExample } from "./examples/structured-agents";

const meta = {
  title: "Assistant/Generative UI/RecommendationCard",
  parameters: {
    layout: "fullscreen",
    docs: {
      description: {
        component:
          "Swarmie-owned recommendationcard presentation contracts. All examples use synthetic data; applications supply execution and transport through callbacks.",
      },
    },
  },
} satisfies Meta;
export default meta;
type Story = StoryObj<typeof meta>;

export const RecommendationCard: Story = {
  name: "Recommendation card",
  render: () => (
    <ExampleFrame>
      <StructuredAgentExample slug="recommendation-card" />
    </ExampleFrame>
  ),
};
