import type { Meta, StoryObj } from "@storybook/react-vite";

import { ExampleFrame } from "./examples/fixtures";
import { StructuredAgentExample } from "./examples/structured-agents";

const meta = {
  title: "Assistant/Sources & attachments/ArtifactCard",
  parameters: {
    layout: "fullscreen",
    docs: {
      description: {
        component:
          "Swarmie-owned artifactcard presentation contracts. All examples use synthetic data; applications supply execution and transport through callbacks.",
      },
    },
  },
} satisfies Meta;
export default meta;
type Story = StoryObj<typeof meta>;

export const ArtifactCard: Story = {
  name: "Artifact card",
  render: () => (
    <ExampleFrame>
      <StructuredAgentExample slug="artifact-card" />
    </ExampleFrame>
  ),
};
