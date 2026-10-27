import type { Meta, StoryObj } from "@storybook/react-vite";

import { Icon } from "./icon";
import type { IconName } from "./icon";

const catalog = [
  "workspaces",
  "home",
  "chat",
  "task",
  "project",
  "agent",
  "memory",
  "people",
  "mention",
  "microphone",
  "search",
  "bell",
  "inbox",
  "file",
  "grid",
  "settings",
  "panel",
  "sidebar",
  "pin",
  "clock",
  "history",
  "menu",
  "chevron",
  "arrow",
  "back",
  "arrows-exchange",
  "plus",
  "close",
  "stop",
  "mail",
  "key",
  "lock",
  "eye",
  "eye-off",
  "world",
  "check",
  "circle-check",
  "circle-attention",
  "triangle-attention",
  "circle-close",
] as const satisfies readonly IconName[];

const meta = {
  title: "Atoms/Icon",
  component: Icon,
  args: { name: "chat" },
  parameters: {
    docs: {
      description: {
        component:
          "A bounded catalog of Lucide outline icons with a 24-unit grid, rounded terminals, a 1.6-unit stroke, and a 20px default size. Icons inherit text color and are decorative; the adjacent control or label supplies their accessible meaning. Native SVG attributes and refs pass through. Use size utilities for compact or larger contexts.",
      },
    },
  },
} satisfies Meta<typeof Icon>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};

export const Compact: Story = { args: { name: "check", className: "size-4" } };

export const Catalog: Story = {
  render: () => (
    <ul className="grid grid-cols-2 gap-x-6 gap-y-4 sm:grid-cols-4">
      {catalog.map((name) => (
        <li key={name} className="flex items-center gap-3 text-ui">
          <Icon name={name} />
          <span>{name}</span>
        </li>
      ))}
    </ul>
  ),
};

export const SizesAndThemes: Story = {
  render: () => (
    <div className="grid gap-4 sm:grid-cols-2">
      {(["light", "dark"] as const).map((theme) => (
        <div key={theme} data-theme={theme} className="rounded-panel bg-canvas p-6 text-ink">
          <p className="mb-5 text-ui font-medium">{theme === "light" ? "Light" : "Dark"}</p>
          <div className="flex items-start gap-6">
            <div className="grid justify-items-center gap-3">
              <Icon name="chat" className="size-4" />
              <span className="text-xs text-text-muted">16px</span>
            </div>
            <div className="grid justify-items-center gap-3">
              <Icon name="chat" />
              <span className="text-xs text-text-muted">20px</span>
            </div>
            <div className="grid justify-items-center gap-3">
              <Icon name="chat" className="size-6" />
              <span className="text-xs text-text-muted">24px</span>
            </div>
          </div>
          <div className="mt-6 flex items-center gap-5 text-primary">
            <Icon name="workspaces" />
            <Icon name="search" />
            <Icon name="key" />
            <Icon name="world" />
            <Icon name="circle-check" />
          </div>
        </div>
      ))}
    </div>
  ),
};
