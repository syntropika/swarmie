import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, userEvent, within } from "storybook/test";

import { officialExampleSource } from "../../.storybook/example-sources";
import { OfficialExample as HeroUIDocumentationExample } from "../../.storybook/examples/toggle-button-group";
import { ToggleButton } from "./toggle-button";
import { ToggleButtonGroup } from "./toggle-button-group";

const meta = {
  title: "Atoms/ToggleButtonGroup",
  parameters: {
    docs: {
      description: {
        component:
          "[Official HeroUI example](https://heroui.com/en/docs/react/components/toggle-button-group). Uses the upstream compound API with the shared Swarmie theme.",
      },
    },
  },
} satisfies Meta;
export default meta;
type Story = StoryObj<typeof meta>;
export const Default: Story = {
  parameters: {
    docs: { source: { code: officialExampleSource("toggle-button-group"), language: "tsx" } },
  },
  render: () => <HeroUIDocumentationExample />,
};

export const Appearance: Story = {
  render: () => (
    <ToggleButtonGroup
      aria-label="Appearance"
      selectionMode="single"
      disallowEmptySelection
      defaultSelectedKeys={["system"]}
      size="sm"
    >
      <ToggleButton id="system">System</ToggleButton>
      <ToggleButton id="dark">Dark</ToggleButton>
      <ToggleButton id="light">Light</ToggleButton>
    </ToggleButtonGroup>
  ),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    const system = canvas.getByRole("radio", { name: "System" });
    const dark = canvas.getByRole("radio", { name: "Dark" });
    await expect(system).toHaveAttribute("aria-checked", "true");
    await userEvent.click(dark);
    await expect(dark).toHaveAttribute("aria-checked", "true");
    await expect(system).toHaveAttribute("aria-checked", "false");
    await userEvent.click(dark);
    await expect(dark).toHaveAttribute("aria-checked", "true");
  },
};
