import type { Meta, StoryObj } from "@storybook/react-vite";

import { ExampleFrame } from "./examples/fixtures";
import { StructuredAgentExample } from "./examples/structured-agents";

const meta = {
  title: "Assistant/Agents & tasks/JobProgress",
  parameters: {
    layout: "fullscreen",
    docs: {
      description: {
        component:
          "Swarmie-owned jobprogress presentation contracts. All examples use synthetic data; applications supply execution and transport through callbacks.",
      },
    },
  },
} satisfies Meta;
export default meta;
type Story = StoryObj<typeof meta>;

export const JobProgress: Story = {
  name: "Job progress",
  render: () => (
    <ExampleFrame>
      <StructuredAgentExample slug="job-progress" />
    </ExampleFrame>
  ),
};
