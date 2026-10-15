import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, userEvent, waitFor, within } from "storybook/test";

import { ExampleFrame } from "./examples/fixtures";
import { InlineCitations } from "./inline-citations";

const meta = {
  title: "Assistant/Sources & attachments/InlineCitations",
  component: InlineCitations,
  parameters: { layout: "fullscreen" },
} satisfies Meta<typeof InlineCitations>;
export default meta;
type Story = StoryObj;

export const Default: Story = {
  render: () => (
    <ExampleFrame>
      <InlineCitations
        text="Transformers scale well with data and compute[1], though attention is quadratic in sequence length[2]."
        sourcesLabel="Sources"
        references={[
          {
            number: 1,
            label: "Attention Is All You Need",
            description: "A research paper with architecture and evaluation details.",
            host: "arxiv.org",
            url: "https://arxiv.org/abs/1706.03762",
          },
          {
            number: 2,
            label: "Efficient Transformers: A Survey",
            description: "A research paper with architecture and evaluation details.",
            host: "arxiv.org",
            url: "https://arxiv.org/abs/2009.06732",
          },
        ]}
      />
    </ExampleFrame>
  ),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await userEvent.tab();
    await expect(canvas.getByRole("link", { name: "1: Attention Is All You Need" })).toHaveFocus();
    await waitFor(async () => {
      await expect(
        canvas.getByRole("tooltip", { name: "Attention Is All You Need" }),
      ).toBeVisible();
    });
    await expect(canvas.getByRole("navigation", { name: "Sources" })).toBeVisible();
  },
};
