import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, fireEvent, userEvent, waitFor, within } from "storybook/test";

import { ConnectedRendererExample } from "./examples/connected-renderers";
import { ExampleFrame } from "./examples/fixtures";

const meta = {
  title: "Assistant/Content & data/MermaidDiagram",
  parameters: {
    layout: "fullscreen",
    docs: {
      description: {
        component:
          "Swarmie-owned mermaiddiagram presentation contracts. All examples use synthetic data; applications supply execution and transport through callbacks.",
      },
    },
  },
} satisfies Meta;
export default meta;
type Story = StoryObj<typeof meta>;

export const MermaidDiagram: Story = {
  name: "Mermaid diagram",
  render: () => (
    <ExampleFrame>
      <ConnectedRendererExample slug="mermaid-diagram" />
    </ExampleFrame>
  ),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    const diagram = await canvas.findByRole(
      "img",
      { name: "Request to decision flow" },
      { timeout: 10_000 },
    );
    await waitFor(async () => {
      await expect(diagram).toBeVisible();
    });
    await expect(canvas.queryByText("The diagram is still incomplete")).not.toBeInTheDocument();
    await userEvent.click(canvas.getByRole("button", { name: "Zoom in" }));
    await expect(canvas.getByRole("status")).toHaveTextContent("125%");
    await userEvent.click(canvas.getByRole("button", { name: "Expand diagram" }));
    const page = within(canvasElement.ownerDocument.body);
    const dialog = await page.findByRole("dialog");
    await waitFor(async () => {
      await expect(within(dialog).getByText("125%")).toBeVisible();
    });
    await userEvent.click(within(dialog).getByRole("button", { name: "Reset view" }));
    await expect(within(dialog).getByText("100%")).toBeVisible();
    await fireEvent(dialog, new Event("cancel", { cancelable: true }));
    await waitFor(async () => {
      await expect(page.queryByRole("dialog")).not.toBeInTheDocument();
    });
  },
};
