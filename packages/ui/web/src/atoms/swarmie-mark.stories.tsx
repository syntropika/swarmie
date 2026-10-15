import type { Meta, StoryObj } from "@storybook/react-vite";

import { SwarmieMark } from "./swarmie-mark";

const meta = {
  title: "Atoms/SwarmieMark",
  component: SwarmieMark,
  args: { className: "size-12" },
  parameters: {
    docs: {
      description: {
        component:
          "Decorative brand mark for app and web. The surrounding control or visible wordmark supplies its accessible name. Use the on-primary tone on a filled primary surface, the inverse tone on an inverse surface, and the footer tone on the site footer.",
      },
    },
  },
} satisfies Meta<typeof SwarmieMark>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Primary: Story = {};

export const OnBlue: Story = {
  render: () => (
    <div className="w-fit rounded-panel bg-primary p-6 text-on-primary">
      <SwarmieMark tone="on-primary" className="size-12" />
    </div>
  ),
};

export const OnInverse: Story = {
  render: () => (
    <div className="w-fit rounded-panel bg-inverse p-6 text-inverse-text">
      <SwarmieMark tone="inverse" className="size-12" />
    </div>
  ),
};

export const OnFooter: Story = {
  render: () => (
    <div className="w-fit rounded-panel bg-footer p-6 text-footer-text">
      <SwarmieMark tone="footer" className="size-12" />
    </div>
  ),
};
