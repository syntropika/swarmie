import type { Meta, StoryObj } from "@storybook/react-vite";

import { officialExampleSource } from "../../.storybook/example-sources";
import { OfficialExample as HeroUIDocumentationExample } from "../../.storybook/examples/button";
import { Button } from "./button";
import { Icon } from "./icon";
import { Spinner } from "./spinner";

const meta = {
  title: "Atoms/Button",
  component: Button,
  args: { children: "Invite people" },
  parameters: {
    docs: {
      description: {
        component:
          "Direct HeroUI Button export. The vendor owns variants, sizing, focus and pending behavior.",
      },
    },
  },
} satisfies Meta<typeof Button>;
export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {
  parameters: { docs: { source: { code: officialExampleSource("button"), language: "tsx" } } },
  render: () => <HeroUIDocumentationExample />,
};
export const Primary: Story = {};
export const Variants: Story = {
  render: () => (
    <div className="flex flex-wrap items-center gap-3">
      <Button>Invite people</Button>
      <Button variant="secondary">View details</Button>
      <Button variant="tertiary">Cancel</Button>
      <Button variant="ghost">More options</Button>
      <Button variant="outline">Preview</Button>
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
export const IconOnly: Story = {
  args: { isIconOnly: true, "aria-label": "Add project", children: <Icon name="plus" /> },
};
export const Disabled: Story = { args: { isDisabled: true } };
export const Pending: Story = {
  args: {
    isPending: true,
    isDisabled: true,
    children: (
      <>
        <Spinner size="sm" color="current" aria-hidden="true" />
        Saving changes
      </>
    ),
  },
};
