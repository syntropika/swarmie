import type { Meta, StoryObj } from "@storybook/react-vite";

import { ExampleFrame } from "./examples/fixtures";
import { ToolsKnowledgeExample } from "./examples/tools-knowledge";

const meta = {
  title: "Assistant/Sources & attachments/FileTree",
  parameters: {
    layout: "fullscreen",
    docs: {
      description: {
        component:
          "Swarmie-owned filetree presentation contracts. All examples use synthetic data; applications supply execution and transport through callbacks.",
      },
    },
  },
} satisfies Meta;
export default meta;
type Story = StoryObj<typeof meta>;

export const FileTree: Story = {
  name: "File tree",
  render: () => (
    <ExampleFrame>
      <ToolsKnowledgeExample slug="file-tree" />
    </ExampleFrame>
  ),
};
