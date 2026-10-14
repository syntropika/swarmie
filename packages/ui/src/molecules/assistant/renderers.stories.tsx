import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, userEvent, waitFor, within } from "storybook/test";

import { ConnectedRendererExample } from "./examples/connected-renderers";
import { ExampleFrame } from "./examples/fixtures";

const meta = {
  title: "Assistant/Content & data/Text & code",
  parameters: {
    layout: "fullscreen",
    docs: {
      description: {
        component:
          "Swarmie-owned text & code presentation contracts. All examples use synthetic data; applications supply execution and transport through callbacks.",
      },
    },
  },
} satisfies Meta;
export default meta;
type Story = StoryObj<typeof meta>;

export const MarkdownText: Story = {
  name: "Markdown text",
  render: () => (
    <ExampleFrame>
      <ConnectedRendererExample slug="markdown-text" />
    </ExampleFrame>
  ),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    const user = userEvent.setup({ document: canvasElement.ownerDocument });
    await user.click(canvas.getByRole("button", { name: "Copy code" }));
    await waitFor(async () => {
      await expect(canvas.getByText("Code copied")).toBeInTheDocument();
    });
    await expect(
      canvasElement.ownerDocument.defaultView?.navigator.clipboard.readText(),
    ).resolves.toBe('const decision = "review";\n');
  },
};

export const SyntaxHighlighter: Story = {
  name: "Syntax highlighter",
  render: () => (
    <ExampleFrame>
      <ConnectedRendererExample slug="syntax-highlighter" />
    </ExampleFrame>
  ),
};

export const ShikiHighlighter: Story = {
  name: "Shiki highlighter",
  render: () => (
    <ExampleFrame>
      <ConnectedRendererExample slug="shiki-highlighter" />
    </ExampleFrame>
  ),
};
