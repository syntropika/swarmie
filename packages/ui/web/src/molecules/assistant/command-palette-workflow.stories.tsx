import type { Meta, StoryObj } from "@storybook/react-vite";
import { useState } from "react";
import { expect, userEvent, within } from "storybook/test";

import { CommandPalette } from "./composer";

const meta = {
  title: "Assistant/Composer/Command palette workflow",
  parameters: { layout: "padded" },
} satisfies Meta;
export default meta;
type Story = StoryObj<typeof meta>;

function PaletteExample() {
  const [selected, setSelected] = useState("");
  return (
    <>
      <CommandPalette
        label="Commands"
        placeholder="Search"
        emptyLabel="No commands"
        onSelect={setSelected}
        items={[
          { id: "disabled", label: "Disabled", disabled: true },
          { id: "first", label: "First", group: "Workspace" },
          { id: "last", label: "Last", group: "Workspace" },
        ]}
      />
      <output>{selected}</output>
    </>
  );
}

export const PaletteKeyboard: Story = {
  render: () => <PaletteExample />,
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await userEvent.click(canvas.getByRole("combobox"));
    await userEvent.keyboard("{ArrowUp}{Enter}");
    await expect(canvas.getByText("last")).toBeVisible();
    await userEvent.keyboard("{ArrowDown}{Enter}");
    await expect(canvas.getByText("first")).toBeVisible();
  },
};
