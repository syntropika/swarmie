import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, userEvent, waitFor, within } from "storybook/test";

import { ExampleFrame } from "./examples/fixtures";
import { ThreadWorkspaceExample } from "./examples/thread-workspace";

const meta = {
  title: "Assistant/Conversations/Thread workspace",
  parameters: { layout: "fullscreen" },
} satisfies Meta;
export default meta;
type Story = StoryObj<typeof meta>;

export const Floating: Story = {
  render: () => (
    <ExampleFrame>
      <ThreadWorkspaceExample />
    </ExampleFrame>
  ),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await userEvent.click(canvas.getByRole("button", { name: "Open assistant workspace" }));
    const dialog = canvas.getByRole("dialog", { name: "Workspace assistant" });
    const workspace = within(dialog);
    await expect(workspace.getByRole("textbox", { name: "Message" })).toHaveFocus();
    await userEvent.click(workspace.getByRole("button", { name: "Browse conversations" }));
    await userEvent.click(workspace.getByRole("button", { name: /Research notes/u }));
    await waitFor(async () => {
      await expect(
        workspace.getByText("Earlier notes: keep execution in the host adapter."),
      ).toBeVisible();
    });
    await userEvent.click(workspace.getByRole("button", { name: "New conversation" }));
    await expect(workspace.getByText("How can I help?")).toBeVisible();
    const resize = workspace.getByRole("button", { name: "Resize assistant workspace" });
    resize.focus();
    await userEvent.keyboard("{Enter}");
    const initial = Number(
      dialog.style.getPropertyValue("--assistant-workspace-width").replace("px", ""),
    );
    await userEvent.keyboard("{ArrowLeft}");
    const expandedWidth = Math.min(initial + 16, Math.max(1, window.innerWidth - 32));
    await expect(
      Number(dialog.style.getPropertyValue("--assistant-workspace-width").replace("px", "")),
    ).toBe(expandedWidth);
    await expect(
      JSON.parse(localStorage.getItem("swarmie.storybook.workspace-size") ?? "null"),
    ).toMatchObject({ width: expandedWidth });
    await userEvent.keyboard("{Enter}{Escape}");
    await waitFor(async () => {
      await expect(canvas.queryByRole("dialog")).not.toBeInTheDocument();
    });
    await expect(canvas.getByRole("button", { name: "Open assistant workspace" })).toHaveFocus();
  },
};

export const Embedded: Story = {
  render: () => (
    <ExampleFrame>
      <ThreadWorkspaceExample presentation="embedded" />
    </ExampleFrame>
  ),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await userEvent.click(canvas.getByRole("button", { name: "Collapse conversations" }));
    await expect(
      canvas.queryByRole("navigation", { name: "Conversations" }),
    ).not.toBeInTheDocument();
    await userEvent.click(canvas.getByRole("button", { name: "Show conversations" }));
    await expect(canvas.getByRole("button", { name: /Research notes/u })).toBeVisible();
  },
};

export const ControlledWorkflow: Story = {
  render: () => (
    <ExampleFrame>
      <ThreadWorkspaceExample presentation="thread" />
    </ExampleFrame>
  ),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await userEvent.type(
      canvas.getByRole("textbox", { name: "Message" }),
      "Check the keyboard controls",
    );
    await userEvent.click(canvas.getByRole("button", { name: "Send message" }));
    await expect(canvas.getByRole("group", { name: "Read workspace files?" })).toBeVisible();
    await userEvent.click(canvas.getByRole("button", { name: "Allow once" }));
    await expect(canvas.getByRole("button", { name: "Stop response" })).toBeEnabled();
    await waitFor(async () => {
      await expect(canvas.getByText("Checking", { exact: true })).toBeVisible();
    });
    await expect(canvas.queryByText("navigation", { exact: true })).not.toBeInTheDocument();
    await userEvent.click(canvas.getByRole("button", { name: "Stop response" }));
    await expect(canvas.getByRole("button", { name: "Send message" })).toBeDisabled();
    await userEvent.click(canvas.getByRole("button", { name: "Fail response" }));
    await userEvent.click(canvas.getByRole("button", { name: "Retry response" }));
    await expect(canvas.getByRole("button", { name: "Stop response" })).toBeEnabled();
    await userEvent.click(canvas.getByRole("button", { name: "Finish response" }));
    await waitFor(async () => {
      await expect(
        canvas.getByText("The component has accessible controls and shared styling."),
      ).toBeVisible();
    });
  },
};
