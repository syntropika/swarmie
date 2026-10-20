import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, within } from "storybook/test";

import { LoadingStatus } from "./loading-status";

const meta = {
  title: "Atoms/LoadingStatus",
  component: LoadingStatus,
  args: { children: "Loading messages…" },
  parameters: {
    docs: {
      description: {
        component:
          "A compact status for real pending work. Compose a visible, localized label as children; the consumer owns placement and when it appears. The decorative spinner remains still when reduced motion is preferred.",
      },
    },
  },
} satisfies Meta<typeof LoadingStatus>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {
  play: async ({ canvasElement }) => {
    const status = within(canvasElement).getByRole("status");
    await expect(status).toHaveAttribute("aria-atomic", "true");
    await expect(status).toHaveTextContent("Loading messages…");
  },
};

export const InContent: Story = {
  render: () => (
    <section aria-label="Messages" className="max-w-sm space-y-4 p-4">
      <h2 className="text-lg font-medium text-ink">Messages</h2>
      <LoadingStatus>Loading earlier messages…</LoadingStatus>
    </section>
  ),
};
