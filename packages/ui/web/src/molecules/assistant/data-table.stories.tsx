import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, userEvent, within } from "storybook/test";

import { ExampleFrame } from "./examples/fixtures";
import { StructuredAgentExample } from "./examples/structured-agents";

const meta = {
  title: "Assistant/Content & data/DataTable",
  parameters: {
    layout: "fullscreen",
    docs: {
      description: {
        component:
          "Swarmie-owned datatable presentation contracts. All examples use synthetic data; applications supply execution and transport through callbacks.",
      },
    },
  },
} satisfies Meta;
export default meta;
type Story = StoryObj<typeof meta>;

export const DataTable: Story = {
  name: "Data table",
  render: () => (
    <ExampleFrame>
      <StructuredAgentExample slug="data-table" />
    </ExampleFrame>
  ),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    const family = canvas.getByRole("button", { name: "Family" });
    await userEvent.click(family);
    await expect(canvas.getByRole("columnheader", { name: "Family" })).toHaveAttribute(
      "aria-sort",
      "ascending",
    );
    await userEvent.click(family);
    await expect(canvas.getByRole("columnheader", { name: "Family" })).toHaveAttribute(
      "aria-sort",
      "descending",
    );
    await userEvent.click(family);
    await expect(canvas.getByRole("columnheader", { name: "Family" })).not.toHaveAttribute(
      "aria-sort",
    );
    await expect(canvas.getByText("Original order")).toBeInTheDocument();
  },
};
