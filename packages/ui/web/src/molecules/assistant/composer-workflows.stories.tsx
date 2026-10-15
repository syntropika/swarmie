import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, userEvent, waitFor, within } from "storybook/test";

import { ComposerThreadExample } from "./examples/composer-thread";

const meta = {
  title: "Assistant/Composer/Draft & context workflows",
  parameters: { layout: "padded" },
} satisfies Meta;
export default meta;
type Story = StoryObj<typeof meta>;

export const DraftRestore: Story = {
  render: () => <ComposerThreadExample slug="draft-restore" />,
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await userEvent.click(canvas.getByRole("button", { name: "Restore draft" }));
    await expect(canvas.getByRole("textbox", { name: "Message" })).toHaveValue(
      "Review the open design questions.",
    );
    await expect(canvas.getByRole("textbox", { name: "Message" })).toHaveFocus();
    await expect(canvas.queryByRole("button", { name: "Restore draft" })).not.toBeInTheDocument();
  },
};

export const ContextPopup: Story = {
  render: () => <ComposerThreadExample slug="composer-context" />,
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    const trigger = canvas.getByRole("button", { name: "Session context: 33%" });
    await userEvent.hover(trigger);
    const page = within(canvasElement.ownerDocument.body);
    await waitFor(async () => {
      await expect(page.getByRole("tooltip")).toBeVisible();
    });
    await expect(within(page.getByRole("tooltip")).getByText("System")).toBeVisible();
    await userEvent.keyboard("{Escape}");
    await waitFor(async () => {
      await expect(page.queryByRole("tooltip")).not.toBeInTheDocument();
    });
    await userEvent.unhover(trigger);
    trigger.focus();
    await waitFor(async () => {
      await expect(page.getByRole("tooltip")).toBeVisible();
    });
  },
};
