import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, userEvent, within } from "storybook/test";

import { ConnectedRendererExample } from "./examples/connected-renderers";
import { ExampleFrame } from "./examples/fixtures";

const meta = {
  title: "Assistant/Composer/Selectors & context",
  parameters: {
    layout: "fullscreen",
    docs: {
      description: {
        component:
          "Swarmie-owned selectors & context presentation contracts. All examples use synthetic data; applications supply execution and transport through callbacks.",
      },
    },
  },
} satisfies Meta;
export default meta;
type Story = StoryObj<typeof meta>;

export const ContextDisplay: Story = {
  name: "Context display",
  render: () => (
    <ExampleFrame>
      <ConnectedRendererExample slug="context-display" />
    </ExampleFrame>
  ),
};

export const ModelSelector: Story = {
  name: "Model selector",
  render: () => (
    <ExampleFrame>
      <ConnectedRendererExample slug="model-selector" />
    </ExampleFrame>
  ),
};

export const ComposerTriggerPopover: Story = {
  name: "Composer trigger popover",
  render: () => (
    <ExampleFrame>
      <ConnectedRendererExample slug="composer-trigger-popover" />
    </ExampleFrame>
  ),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await userEvent.click(canvas.getByRole("button", { name: /Alex Design collaborator/u }));
    await expect(canvas.getByRole("textbox", { name: "Message" })).toHaveValue("Ask @Alex ");
    await expect(canvas.getByRole("textbox", { name: "Message" })).toHaveFocus();
    await expect(canvas.queryByRole("region", { name: "Mention a participant" })).toBeNull();
  },
};

export const DirectiveText: Story = {
  name: "Directive text",
  render: () => (
    <ExampleFrame>
      <ConnectedRendererExample slug="directive-text" />
    </ExampleFrame>
  ),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await userEvent.tab();
    await expect(canvas.getByRole("button", { name: "@Swarmie" })).toHaveFocus();
    await userEvent.keyboard("{Enter}");
    await expect(canvas.getByText("Demo action: Swarmie profile opened")).toBeVisible();
  },
};
