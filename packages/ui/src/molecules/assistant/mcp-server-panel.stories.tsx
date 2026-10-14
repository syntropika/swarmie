import type { Meta, StoryObj } from "@storybook/react-vite";

import { ExampleFrame } from "./examples/fixtures";
import { ToolsKnowledgeExample } from "./examples/tools-knowledge";

const meta = {
  title: "Assistant/Tools/Server panel",
  parameters: {
    layout: "fullscreen",
    docs: {
      description: {
        component:
          "Swarmie-owned server panel presentation contracts. All examples use synthetic data; applications supply execution and transport through callbacks.",
      },
    },
  },
} satisfies Meta;
export default meta;
type Story = StoryObj<typeof meta>;

export const McpServerPanel: Story = {
  name: "Server panel",
  render: () => (
    <ExampleFrame>
      <ToolsKnowledgeExample slug="mcp-server-panel" />
    </ExampleFrame>
  ),
};
