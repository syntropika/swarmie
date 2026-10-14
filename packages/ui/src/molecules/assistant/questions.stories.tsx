import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, userEvent, within } from "storybook/test";

import { ExampleFrame } from "./examples/fixtures";
import { StructuredAgentExample } from "./examples/structured-agents";

const meta = {
  title: "Assistant/Approvals & forms/Questions",
  parameters: {
    layout: "fullscreen",
    docs: {
      description: {
        component:
          "Swarmie-owned questions presentation contracts. All examples use synthetic data; applications supply execution and transport through callbacks.",
      },
    },
  },
} satisfies Meta;
export default meta;
type Story = StoryObj<typeof meta>;

export const OptionList: Story = {
  name: "Option list",
  render: () => (
    <ExampleFrame>
      <StructuredAgentExample slug="option-list" />
    </ExampleFrame>
  ),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    const option = canvas.getByRole("radio", { name: /Focused component/u });
    await userEvent.click(option);
    await expect(option).toBeChecked();
  },
};

export const QuestionFlow: Story = {
  name: "Question flow",
  render: () => (
    <ExampleFrame>
      <StructuredAgentExample slug="question-flow" />
    </ExampleFrame>
  ),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await expect(canvas.getByRole("button", { name: "Next" })).toBeDisabled();
    await userEvent.click(canvas.getByRole("radio", { name: /Focused component/u }));
    await userEvent.click(canvas.getByRole("button", { name: "Next" }));
    const next = canvas.getByRole("radio", { name: "Interactive Storybook" });
    await expect(next).toHaveFocus();
    await userEvent.click(next);
    await userEvent.click(canvas.getByRole("button", { name: "Submit answers" }));
    await expect(canvas.queryByRole("button", { name: "Submit answers" })).not.toBeInTheDocument();
  },
};
