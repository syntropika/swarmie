import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, userEvent, within } from "storybook/test";

import { SelectionControl } from "./selection-control";

const meta = {
  title: "Atoms/SelectionControl",
  component: SelectionControl,
  args: { type: "checkbox" },
} satisfies Meta<typeof SelectionControl>;
export default meta;
type Story = StoryObj<typeof meta>;

export const NativeChoices: Story = {
  render: () => (
    <form className="grid gap-3 text-ui">
      <label
        htmlFor="choice-notify"
        className="flex min-h-control items-center gap-3 pointer-coarse:min-h-control-touch"
      >
        <SelectionControl id="choice-notify" type="checkbox" name="notify" value="enabled" />
        Notify me about replies
      </label>
      <fieldset className="grid gap-2">
        <legend className="mb-2 font-medium">Response length</legend>
        <label
          htmlFor="choice-brief"
          className="flex min-h-control items-center gap-3 pointer-coarse:min-h-control-touch"
        >
          <SelectionControl
            id="choice-brief"
            type="radio"
            name="length"
            value="brief"
            defaultChecked
          />
          Brief
        </label>
        <label
          htmlFor="choice-detailed"
          className="flex min-h-control items-center gap-3 pointer-coarse:min-h-control-touch"
        >
          <SelectionControl id="choice-detailed" type="radio" name="length" value="detailed" />
          Detailed
        </label>
        <label
          htmlFor="choice-disabled"
          className="flex min-h-control items-center gap-3 pointer-coarse:min-h-control-touch"
        >
          <SelectionControl
            id="choice-disabled"
            type="radio"
            name="length"
            value="disabled"
            disabled
          />
          Unavailable
        </label>
      </fieldset>
    </form>
  ),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    const checkbox = canvas.getByRole("checkbox", { name: "Notify me about replies" });
    await userEvent.tab();
    await expect(checkbox).toHaveFocus();
    await userEvent.keyboard(" ");
    await expect(checkbox).toBeChecked();
    await userEvent.click(canvas.getByRole("radio", { name: "Detailed" }));
    await expect(canvas.getByRole("radio", { name: "Brief" })).not.toBeChecked();
    await expect(canvas.getByRole("radio", { name: "Unavailable" })).toBeDisabled();
    const form = canvasElement.querySelector("form");
    if (!form) {
      throw new Error("The selection fixture requires a form.");
    }
    const submitted = new FormData(form);
    await expect(submitted.get("notify")).toBe("enabled");
    await expect(submitted.get("length")).toBe("detailed");
  },
};
