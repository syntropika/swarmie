import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, userEvent, waitFor, within } from "storybook/test";

import { ExampleFrame } from "./examples/fixtures";
import { ExampleReasoningStream } from "./examples/reasoning-stream";
import { ThinkingReasoning } from "./thinking-reasoning";

const meta = {
  title: "Assistant/Reasoning/ThinkingReasoning",
  component: ThinkingReasoning,
  parameters: { layout: "fullscreen" },
} satisfies Meta<typeof ThinkingReasoning>;
export default meta;
type Story = StoryObj;

export const Default: Story = {
  render: () => (
    <ExampleFrame>
      <ExampleReasoningStream />
    </ExampleFrame>
  ),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await userEvent.click(canvas.getByRole("button", { name: "Finish thinking" }));
    let toggle = canvas.getByRole("button", { name: "Toggle thought" });
    await expect(toggle).toHaveAttribute("aria-expanded", "false");
    await userEvent.click(toggle);
    await expect(toggle).toHaveAttribute("aria-expanded", "true");
    await waitFor(async () => {
      await expect(canvas.getByText(/Preparing the preview/u)).toBeVisible();
    });
    await userEvent.click(canvas.getByRole("button", { name: "Start thinking" }));
    toggle = canvas.getByRole("button", { name: "Toggle thought" });
    await expect(toggle).toBeEnabled();
    await expect(toggle).toHaveAttribute("aria-expanded", "true");
    await expect(canvas.queryByText(/Preparing the preview/u)).not.toBeInTheDocument();
    await userEvent.click(toggle);
    await expect(toggle).toHaveAttribute("aria-expanded", "false");
    await waitFor(async () => {
      await expect(canvas.getByText(/Reading the request/u)).toBeInTheDocument();
    });
    await expect(toggle).toHaveAttribute("aria-expanded", "false");
    await userEvent.click(canvas.getByRole("button", { name: "Finish thinking" }));
    await userEvent.click(toggle);
    await waitFor(async () => {
      await expect(canvas.getByText(/Preparing the preview/u)).toBeVisible();
    });
    await userEvent.keyboard("{Enter}");
    await expect(toggle).toHaveAttribute("aria-expanded", "false");
  },
};
