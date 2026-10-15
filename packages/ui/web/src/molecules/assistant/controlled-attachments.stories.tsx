import type { Meta, StoryObj } from "@storybook/react-vite";

import { ConnectedRendererExample } from "./examples/connected-renderers";
import { ExampleFrame } from "./examples/fixtures";

const meta = {
  title: "Assistant/Sources & attachments/Controlled attachments",
  parameters: {
    layout: "fullscreen",
    docs: {
      description: {
        component:
          "Swarmie-owned controlled attachments presentation contracts. All examples use synthetic data; applications supply execution and transport through callbacks.",
      },
    },
  },
} satisfies Meta;
export default meta;
type Story = StoryObj<typeof meta>;

export const Attachment: Story = {
  name: "Attachment",
  render: () => (
    <ExampleFrame>
      <ConnectedRendererExample slug="attachment" />
    </ExampleFrame>
  ),
};

export const Image: Story = {
  name: "Image",
  render: () => (
    <ExampleFrame>
      <ConnectedRendererExample slug="image" />
    </ExampleFrame>
  ),
};

export const File: Story = {
  name: "File",
  render: () => (
    <ExampleFrame>
      <ConnectedRendererExample slug="file" />
    </ExampleFrame>
  ),
};
