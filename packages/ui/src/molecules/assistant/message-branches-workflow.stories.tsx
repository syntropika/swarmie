import type { Meta, StoryObj } from "@storybook/react-vite";
import { useState } from "react";
import { expect, userEvent, within } from "storybook/test";

import { MessageBranches } from "./messages";

const meta = {
  title: "Assistant/Messages/Branch navigation workflow",
  parameters: { layout: "padded" },
} satisfies Meta;
export default meta;
type Story = StoryObj<typeof meta>;

function BranchExample() {
  const [current, setCurrent] = useState(0);
  return (
    <>
      <MessageBranches
        label="Versions"
        current={current}
        count={3}
        previousLabel="Previous"
        nextLabel="Next"
        onChange={setCurrent}
      />
      <MessageBranches
        label="Empty versions"
        current={0}
        count={0}
        previousLabel="Empty previous"
        nextLabel="Empty next"
        onChange={setCurrent}
      />
    </>
  );
}

export const CyclicBranches: Story = {
  render: () => <BranchExample />,
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await userEvent.click(canvas.getByRole("button", { name: "Previous" }));
    await expect(canvas.getByText("3 / 3")).toBeVisible();
    await userEvent.click(canvas.getByRole("button", { name: "Next" }));
    await expect(canvas.getByText("1 / 3")).toBeVisible();
    await expect(canvas.getByText("0 / 0")).toBeVisible();
    await expect(canvas.getByRole("button", { name: "Empty next" })).toBeDisabled();
  },
};
