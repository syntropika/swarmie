import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, userEvent, within } from "storybook/test";

import { ExampleFrame } from "./examples/fixtures";
import { ToolsKnowledgeExample } from "./examples/tools-knowledge";

const meta = {
  title: "Assistant/Approvals & forms/PermissionGrant",
  parameters: {
    layout: "fullscreen",
    docs: {
      description: {
        component:
          "Swarmie-owned permissiongrant presentation contracts. All examples use synthetic data; applications supply execution and transport through callbacks.",
      },
    },
  },
} satisfies Meta;
export default meta;
type Story = StoryObj<typeof meta>;

export const PermissionGrant: Story = {
  name: "Permission grant",
  render: () => (
    <ExampleFrame>
      <ToolsKnowledgeExample slug="permission-grant" />
    </ExampleFrame>
  ),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await userEvent.click(canvas.getByRole("button", { name: "Always allow" }));
    await expect(canvas.getByText("Granted for always")).toBeVisible();
    await expect(canvas.queryByRole("button", { name: "Decline" })).not.toBeInTheDocument();
  },
};
