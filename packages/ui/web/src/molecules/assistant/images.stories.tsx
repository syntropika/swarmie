import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, fireEvent, userEvent, waitFor, within } from "storybook/test";

import { ExampleFrame } from "./examples/fixtures";
import { ToolsKnowledgeExample } from "./examples/tools-knowledge";

const meta = {
  title: "Assistant/Content & data/Images",
  parameters: {
    layout: "fullscreen",
    docs: {
      description: {
        component:
          "Swarmie-owned images presentation contracts. All examples use synthetic data; applications supply execution and transport through callbacks.",
      },
    },
  },
} satisfies Meta;
export default meta;
type Story = StoryObj<typeof meta>;

export const ImageGeneration: Story = {
  name: "Image generation",
  render: () => (
    <ExampleFrame>
      <ToolsKnowledgeExample slug="image-generation" />
    </ExampleFrame>
  ),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await userEvent.click(canvas.getByRole("button", { name: "Resolve example image" }));
    await expect(canvas.getByRole("img", { name: "Generated illustration fixture" })).toBeVisible();
    await userEvent.click(canvas.getByRole("button", { name: "Regenerate image" }));
    await expect(
      canvas.queryByRole("img", { name: "Generated illustration fixture" }),
    ).not.toBeInTheDocument();
  },
};

export const ImageGallery: Story = {
  name: "Image gallery",
  render: () => (
    <ExampleFrame>
      <ToolsKnowledgeExample slug="image-gallery" />
    </ExampleFrame>
  ),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    const page = within(canvasElement.ownerDocument.body);
    const trigger = canvas.getByRole("button", { name: "Shared document" });
    await userEvent.click(trigger);
    const dialog = await page.findByRole("dialog");
    await expect(within(dialog).getByRole("button", { name: "Previous image" })).toBeDisabled();
    await userEvent.click(within(dialog).getByRole("button", { name: "Next image" }));
    await expect(
      within(dialog).getByRole("img", { name: "Second document illustration" }),
    ).toBeVisible();
    await expect(within(dialog).getByRole("button", { name: "Next image" })).toBeDisabled();
    await userEvent.keyboard("{ArrowLeft}");
    await expect(within(dialog).getByRole("img", { name: "Document illustration" })).toBeVisible();
    await fireEvent(dialog, new Event("cancel", { cancelable: true }));
    await waitFor(async () => {
      await expect(dialog).not.toBeVisible();
    });
    await expect(trigger).toHaveFocus();
  },
};
