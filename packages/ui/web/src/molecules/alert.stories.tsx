import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, within } from "storybook/test";

import { Button } from "../atoms/button";
import { Icon } from "../atoms/icon";
import { AlertRoot, AlertIcon, AlertContent, AlertTitle, AlertDescription } from "./alert";

const meta = {
  title: "Molecules/Alert",
  component: AlertRoot,
  args: {
    tone: "info",
    children: (
      <>
        <AlertIcon>
          <Icon name="circle-attention" />
        </AlertIcon>
        <AlertContent>
          <AlertTitle>Your changes are saved.</AlertTitle>
          <AlertDescription>Everyone with access will see the latest version.</AlertDescription>
        </AlertContent>
      </>
    ),
  },
  decorators: [
    (Story) => (
      <div className="max-w-md">
        <Story />
      </div>
    ),
  ],
  parameters: {
    docs: {
      description: {
        component:
          "Compose optional icon, title, description, and ordinary actions as children. Tone controls presentation and the default live-region role: error is alert, info and success are status. The consumer owns content and when a notice appears.",
      },
    },
  },
} satisfies Meta<typeof AlertRoot>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Information: Story = {
  play: async ({ canvasElement }) => {
    const notice = within(canvasElement).getByRole("status");
    await expect(notice).toHaveAttribute("aria-atomic", "true");
    await expect(notice).toHaveTextContent("Your changes are saved.");
    await expect(notice).toHaveTextContent("Everyone with access will see the latest version.");
  },
};

export const Success: Story = { args: { tone: "success" } };

export const Compact: Story = {
  args: { tone: "success", appearance: "soft", spacing: "compact" },
};

export const PlainFeedback: Story = {
  args: {
    tone: "error",
    appearance: "plain",
    spacing: "none",
    children: <AlertDescription size="body">Enter a valid email address.</AlertDescription>,
  },
};

export const ErrorWithAction: Story = {
  args: {
    tone: "error",
    children: (
      <>
        <AlertIcon>
          <Icon name="circle-attention" />
        </AlertIcon>
        <AlertContent>
          <AlertTitle>We couldn’t load your changes.</AlertTitle>
          <AlertDescription>Check your connection and open the page again.</AlertDescription>
          <Button className="mt-3" variant="secondary" asChild>
            <a href="#retry-example">Open the page</a>
          </Button>
        </AlertContent>
      </>
    ),
  },
  play: async ({ canvasElement }) => {
    const notice = within(canvasElement).getByRole("alert");
    await expect(notice).toHaveAttribute("data-tone", "error");
    await expect(within(notice).getByRole("link", { name: "Open the page" })).toHaveAttribute(
      "href",
      "#retry-example",
    );
    await expect(notice).toHaveTextContent("Check your connection and open the page again.");
  },
};

export const WithoutIcon: Story = {
  args: {
    children: (
      <AlertContent>
        <AlertTitle>All caught up.</AlertTitle>
        <AlertDescription>There are no new updates.</AlertDescription>
      </AlertContent>
    ),
  },
};
