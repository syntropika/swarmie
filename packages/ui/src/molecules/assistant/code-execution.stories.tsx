import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, userEvent, waitFor, within } from "storybook/test";

import { ExampleFrame } from "./examples/fixtures";
import { ToolsKnowledgeExample } from "./examples/tools-knowledge";

const meta = {
  title: "Assistant/Tools/Code execution",
  parameters: {
    layout: "fullscreen",
    docs: {
      description: {
        component:
          "Swarmie-owned code execution presentation contracts. All examples use synthetic data; applications supply execution and transport through callbacks.",
      },
    },
  },
} satisfies Meta;
export default meta;
type Story = StoryObj<typeof meta>;

export const TerminalBlock: Story = {
  name: "Terminal block",
  render: () => (
    <ExampleFrame>
      <ToolsKnowledgeExample slug="terminal-block" />
    </ExampleFrame>
  ),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await expect(canvas.queryByText("All checks completed.")).not.toBeInTheDocument();
    await userEvent.click(canvas.getByRole("button", { name: "Show all lines" }));
    await waitFor(async () => {
      await expect(canvas.getByText("Optional preview was skipped")).toBeVisible();
      await expect(canvas.getByText("All checks completed.")).toBeVisible();
    });
    await userEvent.click(canvas.getByRole("button", { name: "Copy output" }));
    await expect(canvas.getByText(/Demo action: Copied output/u)).toBeVisible();
    await userEvent.click(canvas.getByRole("button", { name: "Show less" }));
    await expect(canvas.queryByText("All checks completed.")).not.toBeInTheDocument();
  },
};

export const CodeRunner: Story = {
  name: "Code runner",
  render: () => (
    <ExampleFrame>
      <ToolsKnowledgeExample slug="code-runner" />
    </ExampleFrame>
  ),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await userEvent.click(canvas.getByRole("button", { name: "Run snippet" }));
    await expect(canvas.getByRole("button", { name: "Run snippet" })).toBeDisabled();
    await userEvent.click(canvas.getByRole("button", { name: "Finish snippet" }));
    await expect(canvas.getByText("3", { exact: true })).toBeVisible();
    await expect(canvas.getByRole("button", { name: "Run snippet" })).toBeEnabled();
  },
};
