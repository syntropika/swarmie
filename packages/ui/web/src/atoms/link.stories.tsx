import type { Meta, StoryObj } from "@storybook/react-vite";

import { officialExampleSource } from "../../.storybook/example-sources";
import { OfficialExample as HeroUIDocumentationExample } from "../../.storybook/examples/link";
import { Link } from "./link";

const meta = {
  title: "Atoms/Link",
  component: Link,
  args: { children: null },
} satisfies Meta<typeof Link>;
export default meta;
type Story = StoryObj<typeof meta>;
export const Default: Story = {
  render: () => <Link href="#destination">Read the details</Link>,
};

export const OfficialExample: Story = {
  parameters: { docs: { source: { code: officialExampleSource("link"), language: "tsx" } } },
  name: "Official example",
  render: () => <HeroUIDocumentationExample />,
};
