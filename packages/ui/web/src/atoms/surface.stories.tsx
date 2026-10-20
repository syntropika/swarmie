import type { Meta, StoryObj } from "@storybook/react-vite";

import { officialExampleSource } from "../../.storybook/example-sources";
import { OfficialExample as HeroUIDocumentationExample } from "../../.storybook/examples/surface";
import { Surface } from "./surface";

const meta = {
  title: "Atoms/Surface",
  component: Surface,
  args: { children: "A semantic surface inherits the shared theme." },
} satisfies Meta<typeof Surface>;
export default meta;
type Story = StoryObj<typeof meta>;
export const Default: Story = {};
export const Secondary: Story = { args: { variant: "secondary" } };
export const Tertiary: Story = { args: { variant: "tertiary" } };
export const Transparent: Story = { args: { variant: "transparent" } };

export const OfficialExample: Story = {
  parameters: { docs: { source: { code: officialExampleSource("surface"), language: "tsx" } } },
  name: "Official example",
  render: () => <HeroUIDocumentationExample />,
};
