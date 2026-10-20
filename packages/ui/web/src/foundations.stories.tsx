import type { Meta, StoryObj } from "@storybook/react-vite";

import { IconsReference, MotionReference } from "./foundations/behavior";
import { ColorReference } from "./foundations/colors";
import { BrandReference, FoundationOverview } from "./foundations/identity";
import { ResponsiveReference } from "./foundations/responsive";
import { SpacingReference, ShadowsReference } from "./foundations/spacing-shadows";
import { TypographyReference } from "./foundations/typography";

const meta = {
  title: "Foundations",
  parameters: { layout: "fullscreen" },
} satisfies Meta;
export default meta;
type Story = StoryObj<typeof meta>;

export const Overview: Story = { render: () => <FoundationOverview /> };
export const Brand: Story = { render: () => <BrandReference /> };
export const ResponsiveDesign: Story = {
  render: (_args, context) => (
    <ResponsiveReference theme={context.globals.theme === "dark" ? "dark" : "light"} />
  ),
};
export const Typography: Story = {
  render: (_args, context) => (
    <TypographyReference theme={context.globals.theme === "dark" ? "dark" : "light"} />
  ),
};
export const Colors: Story = {
  render: (_args, context) => (
    <ColorReference theme={context.globals.theme === "dark" ? "dark" : "light"} />
  ),
};
export const Motion: Story = {
  render: (_args, context) => (
    <MotionReference theme={context.globals.theme === "dark" ? "dark" : "light"} />
  ),
};
export const Icons: Story = {
  render: (_args, context) => (
    <IconsReference theme={context.globals.theme === "dark" ? "dark" : "light"} />
  ),
};

export const Spacing: Story = {
  render: (_args, context) => (
    <SpacingReference theme={context.globals.theme === "dark" ? "dark" : "light"} />
  ),
};
export const Shadows: Story = {
  render: (_args, context) => (
    <ShadowsReference theme={context.globals.theme === "dark" ? "dark" : "light"} />
  ),
};
