import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, userEvent, waitFor, within } from "storybook/test";

import { ExampleFrame } from "./examples/fixtures";
import { ToolsKnowledgeExample } from "./examples/tools-knowledge";

const meta = {
  title: "Assistant/Approvals & forms/ElicitationForm",
  parameters: {
    layout: "fullscreen",
    docs: {
      description: {
        component:
          "Swarmie-owned elicitationform presentation contracts. All examples use synthetic data; applications supply execution and transport through callbacks.",
      },
    },
  },
} satisfies Meta;
export default meta;
type Story = StoryObj<typeof meta>;

export const ElicitationForm: Story = {
  name: "Elicitation form",
  render: () => (
    <ExampleFrame>
      <ToolsKnowledgeExample slug="elicitation-form" />
    </ExampleFrame>
  ),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    const page = within(canvasElement.ownerDocument.body);
    const workspace = canvas.getByRole("textbox", { name: "Workspace" });
    await userEvent.clear(workspace);
    await userEvent.click(canvas.getByRole("button", { name: "Continue" }));
    await expect(canvas.queryByText(/Demo action: Submitted/u)).not.toBeInTheDocument();
    await userEvent.type(workspace, "Design team");
    const trigger = canvas.getByRole("combobox", { name: "Access scope" });
    await userEvent.click(trigger);
    await userEvent.click(await page.findByRole("option", { name: "Read and write" }));
    await waitFor(async () => {
      await expect(trigger).toHaveTextContent("Read and write");
      await expect(trigger).toHaveFocus();
    });
    await userEvent.click(canvas.getByRole("button", { name: "Continue" }));
    await expect(canvas.getByText("Demo action: Submitted true, Design team, write")).toBeVisible();
    await waitFor(async () => {
      await expect(canvas.getByText("Sent to connector")).toBeVisible();
    });
    await expect(workspace).toBeDisabled();
  },
};
