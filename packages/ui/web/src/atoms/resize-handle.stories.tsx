import type { Meta, StoryObj } from "@storybook/react-vite";
import { useState } from "react";
import { expect, userEvent, waitFor, within } from "storybook/test";

import { ResizeHandle } from "../index";

const meta = {
  title: "Atoms/Resize Handle",
  parameters: { layout: "padded" },
  tags: ["autodocs"],
} satisfies Meta;
export default meta;
type Story = StoryObj<typeof meta>;

function Example() {
  const [width, setWidth] = useState(280);
  return (
    <div className="flex items-center gap-4">
      <p>Panel width: {width}</p>
      <ResizeHandle
        aria-label="Panel width"
        aria-valuenow={width}
        aria-valuemin={220}
        aria-valuemax={360}
        className="h-24 w-2 cursor-col-resize rounded-pill bg-border focus-visible:outline-2 focus-visible:outline-focus"
        onKeyDown={(event) => {
          if (event.key === "ArrowRight" || event.key === "ArrowLeft") {
            event.preventDefault();
            setWidth(Math.max(220, Math.min(360, width + (event.key === "ArrowRight" ? 10 : -10))));
          }
        }}
      />
    </div>
  );
}
export const Keyboard: Story = {
  render: () => <Example />,
  play: async ({ canvasElement }) => {
    const control = within(canvasElement).getByRole("separator", { name: "Panel width" });
    control.focus();
    await userEvent.keyboard("{ArrowRight}");
    await waitFor(async () => {
      await expect(control).toHaveAttribute("aria-valuenow", "290");
    });
  },
};
