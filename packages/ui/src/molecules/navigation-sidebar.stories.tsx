import type { Meta, StoryObj } from "@storybook/react-vite";

import { Button } from "../atoms/button";
import { Icon } from "../atoms/icon";
import { NavigationSidebar } from "./navigation-sidebar";

const meta = {
  title: "Molecules/NavigationSidebar",
  component: NavigationSidebar,
  parameters: { layout: "fullscreen" },
  decorators: [
    (Story) => (
      <div className="h-dvh w-72 bg-canvas text-ink">
        <Story />
      </div>
    ),
  ],
} satisfies Meta<typeof NavigationSidebar>;

export default meta;
type Story = StoryObj<typeof meta>;

export const WorkspaceContent: Story = {
  args: {
    label: "Conversations",
    header: <h2 className="min-w-0 flex-1 px-3 text-ui font-medium">Example workspace</h2>,
    children: (
      <nav
        aria-label="Example conversations"
        className="flex min-h-0 flex-1 flex-col gap-1 overflow-y-auto"
      >
        <Button variant="navigation" size="navigation">
          <Icon name="plus" />
          New conversation
        </Button>
        <Button variant="navigation" size="navigation" selected aria-current="page">
          Example conversation
        </Button>
      </nav>
    ),
  },
};

export const SettingsContent: Story = {
  args: {
    label: "Settings",
    header: <h2 className="min-w-0 flex-1 px-3 text-ui font-medium">Settings</h2>,
    children: (
      <nav
        aria-label="Example settings"
        className="flex min-h-0 flex-1 flex-col gap-1 overflow-y-auto"
      >
        <Button variant="navigation" size="navigation" selected aria-current="page">
          <Icon name="settings" />
          General
        </Button>
        <Button variant="navigation" size="navigation">
          <Icon name="eye" />
          Appearance
        </Button>
      </nav>
    ),
  },
};
