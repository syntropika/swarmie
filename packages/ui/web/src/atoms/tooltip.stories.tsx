import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, userEvent, waitFor, within } from "storybook/test";

import { Button } from "./button";
import { Icon } from "./icon";
import { TooltipContent, TooltipProvider, TooltipRoot, TooltipTrigger } from "./tooltip";

const meta = {
  title: "Atoms/Tooltip",
  component: TooltipContent,
  parameters: { layout: "centered" },
} satisfies Meta<typeof TooltipContent>;

export default meta;
type Story = StoryObj<typeof meta>;

export const KeyboardDisclosure: Story = {
  render: () => (
    <TooltipProvider>
      <TooltipRoot>
        <TooltipTrigger asChild>
          <Button variant="ghost" size="icon-sm" aria-label="Explain this control">
            <Icon name="circle-attention" />
          </Button>
        </TooltipTrigger>
        <TooltipContent side="bottom">A short explanation of the control.</TooltipContent>
      </TooltipRoot>
    </TooltipProvider>
  ),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    const document = within(canvasElement.ownerDocument.body);
    const trigger = canvas.getByRole("button", { name: "Explain this control" });
    await userEvent.tab();
    await expect(trigger).toHaveFocus();
    const tooltip = await document.findByRole("tooltip");
    await expect(trigger).toHaveAttribute("aria-describedby", tooltip.id);
    await userEvent.keyboard("{Escape}");
    await waitFor(async () => {
      await expect(document.queryByRole("tooltip")).toBeNull();
    });
    await expect(trigger).toHaveFocus();
  },
};
