import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, userEvent, within } from "storybook/test";

import { Button } from "../atoms/button";
import { Input } from "../atoms/input";
import { Field } from "./field";

const meta = {
  title: "Molecules/Field",
  component: Field,
  args: {
    id: "workspace-name",
    label: "Workspace name",
    helper: "Choose a name your team will recognize.",
    children: (control) => (
      <Input {...control} placeholder="Your workspace" autoComplete="organization" />
    ),
  },
  decorators: [
    (Story) => (
      <div className="w-full max-w-sm">
        <Story />
      </div>
    ),
  ],
  parameters: {
    docs: {
      description: {
        component:
          "Field owns the persistent label and description IDs. Labels are visible by default; screen-reader visibility supports compact controls while keeping any label action visible and keyboard accessible. Spread its render-prop attributes onto exactly one native control. Validation is supplied by the consuming form; the primitive does not infer product rules.",
      },
    },
  },
} satisfies Meta<typeof Field>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};

export const CompactLabel: Story = { args: { labelSize: "compact" } };

export const ValidationError: Story = {
  args: { error: "Enter a workspace name to continue." },
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    const input = canvas.getByRole("textbox", { name: "Workspace name" });
    await expect(input).toHaveAccessibleDescription(
      "Choose a name your team will recognize. Enter a workspace name to continue.",
    );
    await expect(input).toHaveAttribute("aria-invalid", "true");
    await userEvent.click(canvas.getByText("Workspace name"));
    await expect(input).toHaveFocus();
  },
};

export const Disabled: Story = {
  args: {
    helper: "You need editing access to rename this workspace.",
    children: (control) => <Input {...control} defaultValue="Studio" disabled />,
  },
};

export const KeyboardEntry: Story = {
  render: () => (
    <form
      className="flex flex-col items-start gap-5"
      onSubmit={(event) => {
        event.preventDefault();
      }}
    >
      <Field
        id="keyboard-workspace"
        label="Workspace name"
        helper="A name for your shared work."
        className="w-full"
      >
        {(control) => <Input {...control} name="workspace" autoComplete="organization" />}
      </Field>
      <Button type="submit">Save name</Button>
    </form>
  ),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    const input = canvas.getByRole("textbox", { name: "Workspace name" });
    await userEvent.click(canvas.getByText("Workspace name"));
    await userEvent.keyboard("Studio");
    await expect(input).toHaveValue("Studio");
    await userEvent.tab();
    await expect(canvas.getByRole("button", { name: "Save name" })).toHaveFocus();
  },
};

export const LabelAction: Story = {
  args: {
    id: "account-password",
    label: "Password",
    helper: "Use the password for your account.",
    labelAction: (
      <Button variant="link" size="sm">
        Reset password
      </Button>
    ),
    children: (control) => <Input {...control} type="password" autoComplete="current-password" />,
  },
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    const input = canvas.getByLabelText("Password");
    await userEvent.click(canvas.getByText("Password"));
    await expect(input).toHaveFocus();
    await expect(input).toHaveAccessibleDescription("Use the password for your account.");
    await userEvent.tab({ shift: true });
    await expect(canvas.getByRole("button", { name: "Reset password" })).toHaveFocus();
  },
};

export const ScreenReaderLabel: Story = {
  args: {
    id: "message-draft",
    label: "Message",
    labelVisibility: "screen-reader",
    helper: "Write a message for your conversation.",
    error: "Add your message before sending.",
    labelAction: (
      <Button variant="ghost" size="sm">
        Message options
      </Button>
    ),
    children: (control) => (
      <textarea
        {...control}
        name="message"
        placeholder="What would you like to work on?"
        rows={3}
        className="min-h-24 w-full rounded-control border border-transparent bg-surface px-3 py-2 text-body text-ink placeholder:text-text-muted focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-primary"
      />
    ),
  },
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    const input = canvas.getByRole("textbox", { name: "Message" });
    const action = canvas.getByRole("button", { name: "Message options" });
    await expect(input).toHaveAccessibleName("Message");
    await expect(input).toHaveAccessibleDescription(
      "Write a message for your conversation. Add your message before sending.",
    );
    await expect(input).toHaveAttribute(
      "aria-describedby",
      "message-draft-helper message-draft-error",
    );
    await expect(input).toHaveAttribute("aria-invalid", "true");
    await expect(action).toBeVisible();
    await userEvent.click(action);
    await userEvent.tab();
    await expect(input).toHaveFocus();
    await userEvent.keyboard("Plan the next step.");
    await expect(input).toHaveValue("Plan the next step.");
    await expect(input).toHaveAccessibleName("Message");
    await userEvent.tab({ shift: true });
    await expect(action).toHaveFocus();
  },
};
