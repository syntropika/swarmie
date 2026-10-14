import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, userEvent, within } from "storybook/test";

import { ConnectedRendererExample } from "./examples/connected-renderers";
import { ExampleFrame } from "./examples/fixtures";

const meta = {
  title: "Assistant/Tools/Connector configuration",
  parameters: {
    layout: "fullscreen",
    docs: {
      description: {
        component:
          "Swarmie-owned connector configuration presentation contracts. All examples use synthetic data; applications supply execution and transport through callbacks.",
      },
    },
  },
} satisfies Meta;
export default meta;
type Story = StoryObj<typeof meta>;

export const McpConfig: Story = {
  name: "MCP config dialog",
  render: () => (
    <ExampleFrame>
      <ConnectedRendererExample slug="mcp-config" />
    </ExampleFrame>
  ),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await userEvent.click(canvas.getByRole("button", { name: "Configure connectors" }));
    const dialog = within(canvas.getByRole("dialog", { name: "Workspace connectors" }));
    await userEvent.type(dialog.getByLabelText("Authorization token"), "demo-token");
    await userEvent.click(dialog.getByRole("button", { name: "Save demo connector" }));
    await expect(dialog.getByLabelText("Connector URL")).toBeDisabled();
    await expect(dialog.getByLabelText("Authorization token")).toHaveValue("");
    await expect(dialog.getByText("Connecting demo server")).toBeVisible();
    await userEvent.click(dialog.getByRole("button", { name: "Simulate connection failure" }));
    await expect(dialog.getByRole("alert")).toHaveTextContent(
      "The demo server rejected the connection.",
    );
    await userEvent.click(dialog.getByRole("button", { name: "Retry demo connection" }));
    await userEvent.click(dialog.getByRole("button", { name: "Finish demo connection" }));
    await expect(dialog.getByText("Connected in demo")).toBeVisible();
    await expect(dialog.getByText("Streamable HTTP")).toBeVisible();
    await userEvent.click(dialog.getByText("2 tools"));
    await expect(dialog.getByText("Search shared documents")).toBeVisible();
    await userEvent.click(dialog.getByRole("button", { name: "Disconnect demo" }));
    await expect(dialog.getByText("Disconnected")).toBeVisible();
    await userEvent.click(dialog.getByRole("button", { name: "Remove demo connector" }));
    await expect(dialog.getByText("No custom demo connectors.")).toBeVisible();
    await expect(dialog.queryByText("https://example.com/mcp")).not.toBeInTheDocument();
  },
};
