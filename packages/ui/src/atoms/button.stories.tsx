import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, fn, userEvent, within } from "storybook/test";

import { Button } from "./button";
import { Icon } from "./icon";

const meta = {
  title: "Atoms/Button",
  component: Button,
  args: { children: "Invite people", onClick: fn<() => void>() },
  parameters: {
    docs: {
      description: {
        component:
          "Use native buttons for actions and asChild with an anchor for navigation. Native buttons default to type=button; loading retains the label and prevents repeat actions. Tab to a control to inspect its focus ring.",
      },
    },
  },
} satisfies Meta<typeof Button>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Primary: Story = {};

export const FloatingAction: Story = {
  args: {
    variant: "floating",
    size: "icon",
    "aria-label": "Back to latest",
    children: <Icon name="arrow" className="size-5 rotate-90" />,
  },
  play: async ({ canvasElement, args }) => {
    const canvas = within(canvasElement);
    const action = canvas.getByRole("button", { name: "Back to latest" });
    await userEvent.tab();
    await expect(action).toHaveFocus();
    await expect(getComputedStyle(action).outlineStyle).toBe("none");
    await userEvent.keyboard("{Enter}");
    await expect(args.onClick).toHaveBeenCalledTimes(1);
  },
};

export const Variants: Story = {
  render: () => (
    <div className="flex flex-wrap items-center gap-3">
      <Button>Invite people</Button>
      <Button variant="secondary">View details</Button>
      <Button variant="ghost">Cancel</Button>
      <Button variant="destructive">Remove invitation</Button>
    </div>
  ),
};

export const Sizes: Story = {
  render: () => (
    <div className="flex flex-wrap items-center gap-3">
      <Button size="sm">Small</Button>
      <Button size="md">Medium</Button>
      <Button size="lg">Large</Button>
      <Button size="xl">Extra large</Button>
    </div>
  ),
};

export const EntryAction: Story = {
  args: { size: "xl", children: "Create workspace" },
};

export const Navigation: Story = {
  render: () => (
    <div className="flex max-w-sm gap-5">
      <div className="flex flex-col gap-3">
        <Button
          variant="ghost"
          size="icon"
          selected
          aria-pressed="true"
          aria-label="Studio workspace"
        >
          S
        </Button>
        <Button variant="ghost" size="icon" aria-pressed="false" aria-label="Research workspace">
          R
        </Button>
        <Button variant="ghost" size="icon" aria-label="Add workspace">
          <Icon name="plus" />
        </Button>
      </div>
      <div className="flex flex-1 flex-col gap-2">
        <Button variant="navigation" size="navigation" selected aria-current="page">
          Overview
        </Button>
        <Button variant="navigation" size="navigation">
          Conversations
        </Button>
        <Button variant="quiet" size="navigation" disabled>
          Upcoming capability
        </Button>
        <Button variant="navigation" size="account">
          Account settings
        </Button>
      </div>
    </div>
  ),
};

export const CompactActions: Story = {
  render: () => (
    <div className="flex items-center gap-3">
      <Button size="auth">Continue with email</Button>
      <Button variant="link" size="sm">
        Reset password
      </Button>
      <Button
        variant="ghost"
        size="icon-sm"
        selected
        aria-pressed="true"
        aria-label="Mention a participant"
      >
        @
      </Button>
    </div>
  ),
};

export const Disabled: Story = {
  args: { disabled: true },
  play: async ({ canvasElement }) => {
    await expect(within(canvasElement).getByRole("button")).toBeDisabled();
  },
};

export const Loading: Story = {
  args: { loading: true, loadingLabel: "Sending invitations" },
  play: async ({ canvasElement }) => {
    const button = within(canvasElement).getByRole("button", {
      name: /Invite people.*Sending invitations/u,
    });
    await expect(button).toBeDisabled();
    await expect(button).toHaveAttribute("aria-busy", "true");
  },
};

export const Hover: Story = {
  play: async ({ canvasElement }) => {
    await userEvent.hover(within(canvasElement).getByRole("button"));
  },
};

export const KeyboardFocus: Story = {
  play: async ({ canvasElement, args }) => {
    const button = within(canvasElement).getByRole("button");
    button.focus();
    await userEvent.keyboard("{Enter}");
    await expect(button).toHaveFocus();
    await expect(args.onClick).toHaveBeenCalledOnce();
  },
};

export const Link: Story = {
  render: () => (
    <div className="flex max-w-lg flex-col items-start gap-6">
      <Button asChild variant="secondary">
        <a href="#link-example">Read the example</a>
      </Button>
      <p id="link-example" className="text-ui text-text-muted">
        An ordinary anchor keeps native link semantics, destination, and keyboard behavior.
      </p>
    </div>
  ),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await expect(canvas.getByRole("link")).toHaveAttribute("href", "#link-example");
    await expect(canvas.queryByRole("button")).not.toBeInTheDocument();
  },
};

export const DarkWorkingTheme: Story = {
  render: () => (
    <div data-theme="dark" className="flex flex-wrap items-center gap-3 bg-canvas p-6 text-ink">
      <Button>Send message</Button>
      <Button variant="secondary">View details</Button>
      <Button variant="navigation" size="sm" selected aria-current="page">
        Selected session
      </Button>
      <Button variant="destructive">Remove invitation</Button>
      <Button disabled>Unavailable action</Button>
    </div>
  ),
};
