import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, fn, within } from "storybook/test";

import { officialExampleSource } from "../../.storybook/example-sources";
import { OfficialExample as HeroUIDocumentationExample } from "../../.storybook/examples/button";
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
          "HeroUI buttons own action behavior, sizing, and variants. Use Link with a router render function for navigation; loading retains the label and prevents repeat actions. Tab to a control to inspect its focus ring.",
      },
    },
  },
} satisfies Meta<typeof Button>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Primary: Story = {};

export const Brand: Story = { args: { tone: "brand", children: "Connect provider" } };

export const PanelAction: Story = {
  args: { tone: "brand", shape: "panel", size: "lg", children: "Connect provider" },
};

export const FloatingAction: Story = {
  args: {
    variant: "primary",
    isIconOnly: true,
    "aria-label": "Back to latest",
    children: <Icon name="arrow" className="size-5 rotate-90" />,
  },
};

export const Variants: Story = {
  render: () => (
    <div className="flex flex-wrap items-center gap-3">
      <Button>Invite people</Button>
      <Button variant="secondary">View details</Button>
      <Button variant="tertiary">Cancel</Button>
      <Button variant="danger">Remove invitation</Button>
    </div>
  ),
};

export const Sizes: Story = {
  render: () => (
    <div className="flex flex-wrap items-center gap-3">
      <Button size="sm">Small</Button>
      <Button size="md">Medium</Button>
      <Button size="lg">Large</Button>
    </div>
  ),
};

export const Navigation: Story = {
  render: () => (
    <div className="flex max-w-sm gap-5">
      <div className="flex flex-col gap-3">
        <Button variant="tertiary" isIconOnly aria-pressed="true" aria-label="Studio workspace">
          S
        </Button>
        <Button variant="tertiary" isIconOnly aria-pressed="false" aria-label="Research workspace">
          R
        </Button>
        <Button variant="tertiary" isIconOnly aria-label="Add workspace">
          <Icon name="plus" />
        </Button>
      </div>
      <div className="flex flex-1 flex-col gap-2">
        <Button variant="tertiary" size="md" aria-current="page">
          Overview
        </Button>
        <Button variant="tertiary" size="md">
          Conversations
        </Button>
        <Button variant="tertiary" size="md" disabled>
          Upcoming capability
        </Button>
        <Button variant="tertiary" size="lg">
          Account settings
        </Button>
      </div>
    </div>
  ),
};

export const CompactActions: Story = {
  render: () => (
    <div className="flex items-center gap-3">
      <Button size="lg">Continue with email</Button>
      <Button variant="tertiary" size="sm">
        Reset password
      </Button>
      <Button
        variant="tertiary"
        size="sm"
        isIconOnly
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

export const DarkWorkingTheme: Story = {
  render: () => (
    <div data-theme="dark" className="flex flex-wrap items-center gap-3 bg-canvas p-6 text-ink">
      <Button>Send message</Button>
      <Button variant="secondary">View details</Button>
      <Button variant="tertiary" size="sm" aria-current="page">
        Selected session
      </Button>
      <Button variant="danger">Remove invitation</Button>
      <Button disabled>Unavailable action</Button>
    </div>
  ),
};

export const OfficialExample: Story = {
  parameters: { docs: { source: { code: officialExampleSource("button"), language: "tsx" } } },
  name: "Official example",
  render: () => <HeroUIDocumentationExample />,
};
