import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, userEvent, within } from "storybook/test";

import { Button } from "../atoms/button";
import { Icon } from "../atoms/icon";
import { Input } from "../atoms/input";
import { Separator } from "../atoms/separator";
import {
  ActionPopoverContent,
  ActionPopoverItem,
  ActionPopoverIndicator,
  ActionPopoverRoot,
  ActionPopoverTrigger,
  useActionPopover,
} from "./action-popover";

function ExampleActions() {
  const { close } = useActionPopover();
  return (
    <ActionPopoverContent aria-label="Account actions" className="inset-auto top-20 left-6">
      <ActionPopoverItem disabled>
        <Icon name="people" />
        Invite people
      </ActionPopoverItem>
      <ActionPopoverItem onClick={close}>
        <Icon name="settings" />
        Settings
      </ActionPopoverItem>
      <Separator className="my-1" />
      <ActionPopoverItem onClick={close}>
        <Icon name="back" />
        Return to the workspace
      </ActionPopoverItem>
    </ActionPopoverContent>
  );
}

const meta = {
  title: "Molecules/ActionPopover",
  component: ActionPopoverRoot,
  args: { children: null },
  render: () => (
    <div className="flex min-h-72 items-start gap-4 p-6">
      <ActionPopoverRoot>
        <ActionPopoverTrigger aria-label="Open account actions">
          <Icon name="people" />
        </ActionPopoverTrigger>
        <ExampleActions />
      </ActionPopoverRoot>
      <Button variant="secondary">Outside action</Button>
    </div>
  ),
  parameters: {
    docs: {
      description: {
        component:
          "A compact native dialog popover composed from ordinary buttons. The panel supplies shared appearance, first enabled action focus, disclosure state, and focus return; native behavior supplies Escape and light dismissal. Consumers own labels, icons, placement, and action outcomes. Keyboard navigation uses Tab rather than menu widget roles.",
      },
    },
  },
} satisfies Meta<typeof ActionPopoverRoot>;

export default meta;
type Story = StoryObj<typeof meta>;

export const CompactActions: Story = {};

export const FocusAndDismissal: Story = {
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    const trigger = canvas.getByRole("button", { name: "Open account actions" });
    await userEvent.click(trigger);
    const dialog = await canvas.findByRole("dialog", { name: "Account actions" });
    const actions = within(dialog);
    await expect(trigger).toHaveAttribute("aria-expanded", "true");
    await expect(actions.getByRole("button", { name: "Invite people" })).toBeDisabled();
    await expect(actions.getByRole("button", { name: "Settings" })).toHaveFocus();
    await userEvent.tab();
    await expect(actions.getByRole("button", { name: "Return to the workspace" })).toHaveFocus();
    await userEvent.click(actions.getByRole("button", { name: "Return to the workspace" }));
    await expect(trigger).toHaveAttribute("aria-expanded", "false");
    await expect(trigger).toHaveFocus();
  },
};

/** Labeled navigation triggers share the same native disclosure and action contract. */
export const LabeledTrigger: Story = {
  render: () => (
    <div className="flex min-h-72 w-80 items-start p-6">
      <ActionPopoverRoot>
        <ActionPopoverTrigger
          size="navigation"
          variant="disclosure"
          aria-label="Open workspace actions"
        >
          <span className="min-w-0 flex-1 truncate">Workspace</span>
          <ActionPopoverIndicator />
        </ActionPopoverTrigger>
        <ExampleActions />
      </ActionPopoverRoot>
    </div>
  ),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    const trigger = canvas.getByRole("button", { name: "Open workspace actions" });
    trigger.focus();
    await userEvent.keyboard("{Enter}");
    const dialog = await canvas.findByRole("dialog", { name: "Account actions" });
    await expect(dialog).toBeVisible();
    await expect(trigger).toHaveAttribute("aria-expanded", "true");
    await userEvent.keyboard("{Escape}");
    await expect(trigger).toHaveAttribute("aria-expanded", "false");
    await expect(trigger).toHaveFocus();
  },
};

function HoverActions() {
  const { show } = useActionPopover();
  return (
    <>
      <ActionPopoverTrigger
        aria-label="Hover account actions"
        onPointerEnter={() => {
          show({ focus: false });
        }}
      >
        <Icon name="people" />
      </ActionPopoverTrigger>
      <ExampleActions />
    </>
  );
}

export const HoverPreservesEditingFocus: Story = {
  render: () => (
    <div className="flex min-h-72 items-start gap-4 p-6">
      <Input aria-label="Unsent draft" placeholder="Continue writing" />
      <ActionPopoverRoot>
        <HoverActions />
      </ActionPopoverRoot>
    </div>
  ),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    const draft = canvas.getByRole("textbox", { name: "Unsent draft" });
    await userEvent.click(draft);
    await userEvent.type(draft, "Keep this draft");
    await userEvent.hover(canvas.getByRole("button", { name: "Hover account actions" }));
    await expect(await canvas.findByRole("dialog", { name: "Account actions" })).toBeVisible();
    await expect(draft).toHaveFocus();
    await expect(draft).toHaveValue("Keep this draft");
  },
};
