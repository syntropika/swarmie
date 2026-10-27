import type { Meta, StoryObj } from "@storybook/react-vite";

import { Input, Label, ListBox, SearchField, Select, TextArea, TextField } from "../index";

const meta = {
  title: "Foundations/Field surfaces",
  parameters: { layout: "padded" },
} satisfies Meta;
export default meta;
type Story = StoryObj<typeof meta>;

function Fields() {
  return (
    <div className="grid gap-4">
      <TextField defaultValue="Documents">
        <Label>Server name</Label>
        <Input />
      </TextField>
      <SearchField aria-label="Search servers">
        <SearchField.Group>
          <SearchField.SearchIcon />
          <SearchField.Input placeholder="Search servers" />
          <SearchField.ClearButton />
        </SearchField.Group>
      </SearchField>
      <Select defaultValue="current">
        <Label>Protocol version</Label>
        <Select.Trigger>
          <Select.Value />
          <Select.Indicator />
        </Select.Trigger>
        <Select.Popover>
          <ListBox>
            <ListBox.Item id="current" textValue="2026-07-28">
              2026-07-28
              <ListBox.ItemIndicator />
            </ListBox.Item>
          </ListBox>
        </Select.Popover>
      </Select>
      <TextField defaultValue="Summarize the requested changes.">
        <Label>Instructions</Label>
        <TextArea rows={3} />
      </TextField>
    </div>
  );
}

export const CanvasAndPanel: Story = {
  render: () => (
    <div className="@container bg-canvas p-6 text-foreground">
      <div className="grid gap-8 @2xl:grid-cols-2">
        <section className="p-4">
          <h2 className="mb-5 font-medium">Canvas</h2>
          <Fields />
        </section>
        <section className="rounded-xl border border-border bg-surface p-4">
          <h2 className="mb-5 font-medium">Panel</h2>
          <Fields />
        </section>
      </div>
    </div>
  ),
};
