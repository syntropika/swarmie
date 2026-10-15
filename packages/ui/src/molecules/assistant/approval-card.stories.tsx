import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, userEvent, waitFor, within } from "storybook/test";

import { ExampleFrame } from "./examples/fixtures";
import { StructuredAgentExample } from "./examples/structured-agents";

const meta = {
  title: "Assistant/Approvals & forms/ApprovalCard",
  parameters: {
    layout: "fullscreen",
    docs: {
      description: {
        component:
          "Swarmie-owned approvalcard presentation contracts. All examples use synthetic data; applications supply execution and transport through callbacks.",
      },
    },
  },
} satisfies Meta;
export default meta;
type Story = StoryObj<typeof meta>;

export const ApprovalCard: Story = {
  name: "Approval card",
  render: () => (
    <ExampleFrame>
      <StructuredAgentExample slug="approval-card" />
    </ExampleFrame>
  ),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await userEvent.click(canvas.getByRole("button", { name: "Allow once" }));
    await waitFor(async () => {
      await expect(canvas.getByText("Approved, running")).toBeVisible();
    });
    await expect(canvas.queryByRole("button", { name: "Allow once" })).not.toBeInTheDocument();
    await userEvent.click(canvas.getByRole("button", { name: "Finish operation" }));
    await waitFor(async () => {
      await expect(canvas.getByText("Finished")).toBeVisible();
    });
    await userEvent.click(canvas.getByRole("button", { name: "Request again" }));
    await userEvent.click(canvas.getByRole("button", { name: "Decline" }));
    await waitFor(async () => {
      await expect(canvas.getByText("Denied")).toBeVisible();
    });
  },
};
