import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, userEvent, waitFor, within } from "storybook/test";

import { ExampleFrame } from "./examples/fixtures";
import { SourcesExample, ToolsKnowledgeExample } from "./examples/tools-knowledge";

const meta = {
  title: "Assistant/Sources & attachments/Search & knowledge",
  parameters: {
    layout: "fullscreen",
    docs: {
      description: {
        component:
          "Swarmie-owned search & knowledge presentation contracts. All examples use synthetic data; applications supply execution and transport through callbacks.",
      },
    },
  },
} satisfies Meta;
export default meta;
type Story = StoryObj<typeof meta>;

export const WebSearch: Story = {
  name: "Web search",
  render: () => (
    <ExampleFrame>
      <ToolsKnowledgeExample slug="web-search" />
    </ExampleFrame>
  ),
};

export const LinkPreview: Story = {
  name: "Link preview",
  render: () => (
    <ExampleFrame>
      <ToolsKnowledgeExample slug="link-preview" />
    </ExampleFrame>
  ),
};

export const RetrievalChunks: Story = {
  name: "Retrieval chunks",
  render: () => (
    <ExampleFrame>
      <ToolsKnowledgeExample slug="retrieval-chunks" />
    </ExampleFrame>
  ),
};

export const DocumentReference: Story = {
  name: "Document reference",
  render: () => (
    <ExampleFrame>
      <ToolsKnowledgeExample slug="document-reference" />
    </ExampleFrame>
  ),
};

export const MemoryChips: Story = {
  name: "Memory",
  render: () => (
    <ExampleFrame>
      <ToolsKnowledgeExample slug="memory-chips" />
    </ExampleFrame>
  ),
};

export const ResearchReport: Story = {
  name: "Research report",
  render: () => (
    <ExampleFrame>
      <ToolsKnowledgeExample slug="research-report" />
    </ExampleFrame>
  ),
};

export const Sources: Story = {
  name: "Sources",
  render: () => (
    <ExampleFrame>
      <SourcesExample />
    </ExampleFrame>
  ),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    const disclosure = canvas.getByText("Sources", { exact: true }).closest("details");
    await expect(disclosure).not.toHaveAttribute("open");
    await userEvent.click(canvas.getByText("Sources", { exact: true }));
    await expect(disclosure).toHaveAttribute("open");
    const sourceLinks = canvas.getAllByRole("link");
    await expect(sourceLinks.length).toBeGreaterThan(0);
    await Promise.all(
      sourceLinks.map(async (link) => {
        await expect(link).toHaveAttribute("target", "_blank");
      }),
    );
    await userEvent.click(canvas.getByText("Sources", { exact: true }));
    await expect(disclosure).not.toHaveAttribute("open");
    // Native details retains its children during and after the close animation.
    await waitFor(async () => {
      await expect(sourceLinks[0]).not.toBeVisible();
    });
  },
};
