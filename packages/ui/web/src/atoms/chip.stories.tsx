import type { Meta, StoryObj } from "@storybook/react-vite";

import { officialExampleSource } from "../../.storybook/example-sources";
import { OfficialExample as HeroUIDocumentationExample } from "../../.storybook/examples/chip";
import { Chip } from "./chip";
import type { BrandColor } from "./chip";

const meta = {
  title: "Atoms/Chip",
  component: Chip,
  args: { children: "Agent", color: "accent" },
} satisfies Meta<typeof Chip>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};

export const Tones: Story = {
  render: () => (
    <div className="flex flex-wrap items-center gap-3">
      <Chip>Member</Chip>
      <Chip color="accent">Agent</Chip>
      <Chip color="success">Saved</Chip>
      <Chip color="warning">Review needed</Chip>
      <Chip color="danger">Could not save</Chip>
    </div>
  ),
};

const categories = [
  { color: "mint", label: "Research" },
  { color: "lilac", label: "Design" },
  { color: "peach", label: "Community" },
  { color: "mustard", label: "Ideas" },
] satisfies { color: BrandColor; label: string }[];

export const BrandAccents: Story = {
  parameters: {
    docs: {
      description: {
        story:
          "Decorative category colors, independent of success, warning and danger. All sizes and spacing remain HeroUI defaults.",
      },
    },
  },
  render: () => (
    <div className="flex flex-col gap-6">
      {(["primary", "secondary", "soft", "tertiary"] as const).map((variant) => (
        <div key={variant} className="flex flex-col gap-2">
          <span className="text-sm text-muted">{variant}</span>
          <div className="flex flex-wrap gap-3">
            {categories.map(({ color, label }) => (
              <Chip.Root key={color} color={color} variant={variant}>
                <Chip.Label>{label}</Chip.Label>
              </Chip.Root>
            ))}
          </div>
        </div>
      ))}
    </div>
  ),
};

export const OfficialExample: Story = {
  parameters: { docs: { source: { code: officialExampleSource("chip"), language: "tsx" } } },
  name: "Official example",
  render: () => <HeroUIDocumentationExample />,
};
