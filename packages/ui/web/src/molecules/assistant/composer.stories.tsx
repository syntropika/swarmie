import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, userEvent, waitFor, within } from "storybook/test";

import { ComposerThreadExample } from "./examples/composer-thread";
import { ExampleFrame } from "./examples/fixtures";

const meta = {
  title: "Assistant/Composer/Input & controls",
  parameters: {
    layout: "fullscreen",
    docs: {
      description: {
        component:
          "Swarmie-owned React presentation contracts adapted from the Assistant UI composer concepts. All examples use synthetic data; applications supply execution and transport through callbacks.",
      },
    },
  },
} satisfies Meta;
export default meta;
type Story = StoryObj<typeof meta>;

export const Composer: Story = {
  name: "Composer",
  render: () => (
    <ExampleFrame>
      <ComposerThreadExample slug="composer" />
    </ExampleFrame>
  ),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    const input = canvas.getByRole("textbox", { name: "Message" });
    await userEvent.clear(input);
    await expect(canvas.getByRole("button", { name: "Send" })).toBeDisabled();
    await userEvent.type(input, "Review the design{enter}");
    await expect(canvas.getByText("Demo action: Sent: Review the design")).toBeVisible();
    await expect(input).toHaveValue("");
  },
};

export const ComposerSlashCommands: Story = {
  name: "Slash commands",
  render: () => (
    <ExampleFrame>
      <ComposerThreadExample slug="composer-slash-commands" />
    </ExampleFrame>
  ),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    const input = canvas.getByRole("textbox", { name: "Message" });
    const commands = canvas.getByRole("region", { name: "Slash commands" });
    const review = within(commands).getByRole("button", { name: /review a proposal/iu });
    await userEvent.click(input);
    await userEvent.tab();
    await expect(review).toHaveFocus();
    await userEvent.keyboard("{Enter}");
    await waitFor(async () => {
      await expect(input).toHaveValue("/review ");
      await expect(input).toHaveFocus();
      await expect(
        canvas.queryByRole("region", { name: "Slash commands" }),
      ).not.toBeInTheDocument();
    });
    await userEvent.type(input, "the changes{enter}");
    await expect(canvas.getByText("Demo action: Sent: /review the changes")).toBeVisible();
    await expect(input).toHaveValue("");
  },
};

export const ComposerMentions: Story = {
  name: "Mentions",
  render: () => (
    <ExampleFrame>
      <ComposerThreadExample slug="composer-mentions" />
    </ExampleFrame>
  ),
};

export const ComposerAttachments: Story = {
  name: "Attachments",
  render: () => (
    <ExampleFrame>
      <ComposerThreadExample slug="composer-attachments" />
    </ExampleFrame>
  ),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await userEvent.click(canvas.getByRole("button", { name: "Remove: component-guide.md" }));
    await expect(canvas.queryByText("component-guide.md")).not.toBeInTheDocument();
  },
};

export const ComposerModelPicker: Story = {
  name: "Models",
  render: () => (
    <ExampleFrame>
      <ComposerThreadExample slug="composer-model-picker" />
    </ExampleFrame>
  ),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    const page = within(canvasElement.ownerDocument.body);
    const trigger = canvas.getByRole("combobox", { name: "Choose a model" });
    await userEvent.click(trigger);
    const option = await page.findByRole("option", { name: "Deep reasoning" });
    await expect(option).toHaveAccessibleDescription("Complex decisions");
    await waitFor(async () => {
      await expect(page.getByRole("group", { name: "Demo provider" })).toBeVisible();
    });
    await userEvent.click(option);
    await waitFor(async () => {
      await expect(trigger).toHaveTextContent("Deep reasoning");
      await expect(trigger).toHaveFocus();
      await expect(page.queryByRole("listbox")).not.toBeInTheDocument();
    });
    await userEvent.keyboard("{ArrowDown}");
    await waitFor(async () => {
      await expect(page.getByRole("option", { name: "Deep reasoning" })).toHaveFocus();
    });
    await userEvent.keyboard("{End}");
    await waitFor(async () => {
      await expect(page.getByRole("option", { name: "Fast" })).toHaveFocus();
    });
    await userEvent.keyboard("{Enter}");
    await waitFor(async () => {
      await expect(trigger).toHaveTextContent("Fast");
      await expect(trigger).toHaveFocus();
    });
  },
};

export const ComposerVoice: Story = {
  name: "Dictation",
  render: () => (
    <ExampleFrame>
      <ComposerThreadExample slug="composer-voice" />
    </ExampleFrame>
  ),
};

export const ComposerContext: Story = {
  name: "Context",
  render: () => (
    <ExampleFrame>
      <ComposerThreadExample slug="composer-context" />
    </ExampleFrame>
  ),
};

export const DraftRestore: Story = {
  name: "Draft restore",
  render: () => (
    <ExampleFrame>
      <ComposerThreadExample slug="draft-restore" />
    </ExampleFrame>
  ),
};

export const ContextBreakdown: Story = {
  name: "Context breakdown",
  render: () => (
    <ExampleFrame>
      <ComposerThreadExample slug="context-breakdown" />
    </ExampleFrame>
  ),
};

export const PromptLibrary: Story = {
  name: "Prompt library",
  render: () => (
    <ExampleFrame>
      <ComposerThreadExample slug="prompt-library" />
    </ExampleFrame>
  ),
};

export const CommandPalette: Story = {
  name: "Command palette",
  render: () => (
    <ExampleFrame>
      <ComposerThreadExample slug="command-palette" />
    </ExampleFrame>
  ),
};
