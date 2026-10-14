import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, userEvent, waitFor, within } from "storybook/test";

import { ConnectedRendererExample } from "./examples/connected-renderers";
import { ExampleFrame } from "./examples/fixtures";

const meta = {
  title: "Assistant/Tools/Controlled execution",
  parameters: {
    layout: "fullscreen",
    docs: {
      description: {
        component:
          "Swarmie-owned controlled execution presentation contracts. All examples use synthetic data; applications supply execution and transport through callbacks.",
      },
    },
  },
} satisfies Meta;
export default meta;
type Story = StoryObj<typeof meta>;

export const ToolFallback: Story = {
  name: "Tool fallback",
  render: () => (
    <ExampleFrame>
      <ConnectedRendererExample slug="tool-fallback" />
    </ExampleFrame>
  ),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await expect(canvas.getByText(/"path": "packages\/ui"/u)).toBeVisible();
    await userEvent.click(canvas.getByRole("button", { name: "Require approval" }));
    await userEvent.click(canvas.getByRole("button", { name: "Deny inspection" }));
    await expect(canvas.getByText("Inspection denied; no result was produced.")).toBeVisible();
    await expect(
      canvas.queryByRole("button", { name: "Allow inspection once" }),
    ).not.toBeInTheDocument();
    await userEvent.click(canvas.getByRole("button", { name: "Restart demo inspection" }));
    await userEvent.click(canvas.getByRole("button", { name: "Require approval" }));
    await userEvent.click(canvas.getByRole("button", { name: "Allow inspection once" }));
    await expect(canvas.getByText("Allowed this inspection once")).toBeVisible();
    await userEvent.click(canvas.getByRole("button", { name: "Return demo result" }));
    await expect(canvas.getByText(/"count": 2/u)).toBeVisible();
    await userEvent.click(canvas.getByRole("button", { name: "Restart demo inspection" }));
    await userEvent.click(canvas.getByRole("button", { name: "Simulate tool failure" }));
    await waitFor(async () => {
      await expect(
        canvas.getByText("The demo inventory was temporarily unavailable."),
      ).toBeVisible();
    });
    await userEvent.click(canvas.getByRole("button", { name: "Retry inspection" }));
    await waitFor(async () => {
      await expect(canvas.getByText("Retrying inspection")).toBeVisible();
    });
    await userEvent.click(canvas.getByRole("button", { name: "Return demo result" }));
    await expect(canvas.getByText(/"count": 2/u)).toBeVisible();
    await expect(canvas.queryByText("Retrying inspection")).not.toBeInTheDocument();
  },
};

export const ToolGroup: Story = {
  name: "Tool group",
  render: () => (
    <ExampleFrame>
      <ConnectedRendererExample slug="tool-group" />
    </ExampleFrame>
  ),
};
