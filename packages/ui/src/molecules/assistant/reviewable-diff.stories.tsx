import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, userEvent, within } from "storybook/test";

import { ExampleFrame } from "./examples/fixtures";
import { ToolsKnowledgeExample } from "./examples/tools-knowledge";

const meta = {
  title: "Assistant/Content & data/ReviewableDiff",
  parameters: {
    layout: "fullscreen",
    docs: {
      description: {
        component:
          "Swarmie-owned reviewablediff presentation contracts. All examples use synthetic data; applications supply execution and transport through callbacks.",
      },
    },
  },
} satisfies Meta;
export default meta;
type Story = StoryObj<typeof meta>;

export const ReviewableDiff: Story = {
  name: "Reviewable diff",
  render: () => (
    <ExampleFrame>
      <ToolsKnowledgeExample slug="reviewable-diff" />
    </ExampleFrame>
  ),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    const apply = canvas.getByRole("button", { name: "Apply selected changes" });
    await expect(apply).toBeDisabled();
    await userEvent.click(canvas.getByRole("button", { name: "Keep hunk" }));
    await expect(apply).toBeEnabled();
    await userEvent.click(apply);
    await expect(canvas.getByText("Demo action: Applied title")).toBeVisible();
  },
};
