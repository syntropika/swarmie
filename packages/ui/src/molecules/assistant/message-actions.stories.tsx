import type { Meta, StoryObj } from "@storybook/react-vite";
import { useState } from "react";
import { expect, fn, userEvent, within } from "storybook/test";

import { MessageActions } from "./message-actions";
import type { MessageActionsLabels, MessageVote } from "./message-actions";

const labels: MessageActionsLabels = {
  good: "Good response",
  bad: "Bad response",
  copy: "Copy message",
  copied: "Copied",
  react: "Add reaction",
  reaction: (emoji) => `Reaction ${emoji}`,
  reply: "Reply",
  more: "More actions",
  aloud: "Read aloud",
  fork: "Fork chat",
  report: "Report",
  search: "Search emoji",
  empty: "No emoji found",
  back: "Back to quick reactions",
};

function Example({ disabled = false }: { disabled?: boolean }) {
  const [vote, setVote] = useState<MessageVote>(null);
  const [reaction, setReaction] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);
  const [receipt, setReceipt] = useState("");
  return (
    <div className="min-h-96 max-w-xl space-y-3 p-6 pt-64">
      <p className="text-body">The updated contracts keep actions with their caller.</p>
      <MessageActions
        label="Message actions"
        labels={labels}
        text="The updated contracts keep actions with their caller."
        vote={vote}
        onVote={setVote}
        reaction={reaction}
        onReact={setReaction}
        copied={copied}
        onCopy={() => {
          setCopied(true);
          setReceipt("Copy callback received the message text");
        }}
        onReply={() => {
          setReceipt("Reply requested");
        }}
        onAloud={() => {
          setReceipt("Read aloud requested");
        }}
        onFork={() => {
          setReceipt("Fork requested");
        }}
        onReport={() => {
          setReceipt("Report requested");
        }}
        timestamp={{
          dateTime: "2026-10-01T12:00:00Z",
          relative: "3m ago",
          absolute: "October 1, 2026 at 12:00 UTC",
        }}
        disabled={disabled}
      />
      <output className="block text-ui text-text-muted">{receipt}</output>
    </div>
  );
}

const meta = {
  title: "Assistant/Messages/MessageActions",
  component: MessageActions,
  args: {
    label: "Message actions",
    labels,
    text: "Example message",
    vote: null,
    onVote: fn<() => void>(),
    reaction: null,
    onReact: fn<() => void>(),
    copied: false,
    onCopy: fn<() => void>(),
    onReply: fn<() => void>(),
  },
  parameters: {
    docs: {
      description: {
        component:
          "AICSS message actions adapted to Swarmie: toggled feedback, acknowledged copy, quick reactions, the complete searchable emoji catalogue, reply, read aloud, fork, report, and an animated relative timestamp. Native popovers provide light dismissal, Escape, and focus return. Outcomes and labels are supplied by the caller.",
      },
    },
  },
  render: () => <Example />,
} satisfies Meta<typeof MessageActions>;
export default meta;
type Story = StoryObj<typeof meta>;

export const Actions: Story = {
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    const good = canvas.getByRole("button", { name: "Good response" });
    await userEvent.click(good);
    await expect(good).toHaveAttribute("aria-pressed", "true");
    await userEvent.click(good);
    await expect(good).toHaveAttribute("aria-pressed", "false");
    await userEvent.click(canvas.getByRole("button", { name: "Copy message" }));
    await expect(canvas.getByRole("button", { name: "Copied" })).toBeVisible();
    await userEvent.click(canvas.getByRole("button", { name: "More actions" }));
    await userEvent.click(canvas.getByRole("button", { name: "Fork chat" }));
    await expect(canvas.getByText("Fork requested")).toBeVisible();
  },
};

export const SearchableReactions: Story = {
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await userEvent.click(canvas.getByRole("button", { name: "Add reaction" }));
    await userEvent.click(canvas.getByRole("button", { name: "Search emoji" }));
    await userEvent.type(canvas.getByRole("textbox", { name: "Search emoji" }), "rocket");
    const picker = canvas.getByRole("dialog", { name: "Search emoji" });
    await userEvent.click(within(picker).getByRole("button", { name: /rocket/u }));
    await expect(canvas.getByRole("button", { name: "Reaction 🚀" })).toBeVisible();
    await expect(canvas.queryByRole("dialog", { name: "Search emoji" })).not.toBeInTheDocument();
  },
};

export const Disabled: Story = { render: () => <Example disabled /> };
