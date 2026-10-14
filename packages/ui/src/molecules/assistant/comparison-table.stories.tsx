import type { Meta, StoryObj } from "@storybook/react-vite";

import { ComparisonTable } from "./comparison-table";
import { ExampleFrame } from "./examples/fixtures";

const meta = {
  title: "Assistant/Content & data/ComparisonTable",
  component: ComparisonTable,
  parameters: { layout: "fullscreen" },
} satisfies Meta<typeof ComparisonTable>;
export default meta;
type Story = StoryObj;

export const Default: Story = {
  render: () => (
    <ExampleFrame>
      <ComparisonTable
        aria-label="Plan comparison"
        featureLabel="Feature"
        includedLabel="Included"
        excludedLabel="Not included"
        plans={["Personal", "Enterprise"]}
        recommendedPlan="Enterprise"
        recommendationLabel="Recommended"
        reason="Enterprise includes shared team usage and priority support."
        features={[
          { id: "projects", label: "Unlimited projects", values: [true, true] },
          { id: "components", label: "All components", values: [true, true] },
          { id: "team", label: "Team-wide usage", values: [false, true] },
          { id: "support", label: "Priority support", values: [false, true] },
        ]}
      />
    </ExampleFrame>
  ),
};
