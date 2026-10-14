import type { Meta, StoryObj } from "@storybook/react-vite";
import { useState } from "react";
import { expect, userEvent, within } from "storybook/test";

import { ThreadSearch } from "./thread";

const meta = {
  title: "Assistant/Conversations/Navigation workflow",
  parameters: { layout: "padded" },
} satisfies Meta;
export default meta;
type Story = StoryObj<typeof meta>;

function NavigationExample() {
  const [activeId, setActiveId] = useState("none");
  const [action, setAction] = useState("");
  return (
    <>
      <ThreadSearch
        label="Sessions"
        searchLabel="Search sessions"
        activeId={activeId}
        onSelect={setActiveId}
        actionLabel="Manage"
        onAction={setAction}
        items={[
          { id: "old", title: "Old thread", detail: "Design review", group: "Yesterday" },
          {
            id: "pinned",
            title: "Important",
            detail: "Design contract",
            group: "Today",
            pinned: true,
          },
        ]}
      />
      <output>{activeId}</output>
      <output>{action}</output>
    </>
  );
}

export const ThreadNavigation: Story = {
  render: () => <NavigationExample />,
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await userEvent.type(canvas.getByRole("searchbox"), "contract{arrowdown}");
    await expect(canvas.getByText("Design contract")).toBeVisible();
    await expect(canvas.queryByText("Old thread")).not.toBeInTheDocument();
    await userEvent.click(canvas.getByRole("button", { name: "Manage: Important" }));
    await expect(canvas.getAllByText("pinned")).toHaveLength(2);
  },
};
