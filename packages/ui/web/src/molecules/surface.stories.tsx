import type { Meta, StoryObj } from "@storybook/react-vite";

import { Button } from "../atoms/button";
import { Input } from "../atoms/input";
import { Field } from "./field";
import { Surface, SurfaceRow } from "./surface";
import { ThemePreferenceControl } from "./theme-preference-control";

const meta = {
  title: "Molecules/Surface",
  component: Surface,
  decorators: [
    (Story) => (
      <div className="bg-canvas p-6">
        <Story />
      </div>
    ),
  ],
  args: {
    className: "max-w-md",
    children: (
      <section aria-labelledby="surface-title" className="grid gap-5">
        <div>
          <h2 id="surface-title" className="text-title">
            A shared place to work
          </h2>
          <p className="mt-2 text-ui text-text-muted">Keep the task in focus.</p>
        </div>
        <Field id="surface-name" label="Workspace name">
          {(control) => <Input {...control} placeholder="Design studio" />}
        </Field>
        <Button>Create Workspace</Button>
      </section>
    ),
  },
} satisfies Meta<typeof Surface>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Flat: Story = {};
export const Raised: Story = { args: { elevation: "raised" } };
export const Compact: Story = { args: { inset: "compact" } };
export const Composed: Story = {
  args: {
    inset: "none",
    children: (
      <section className="p-6">
        <h2 className="text-title">Consumer-owned layout</h2>
      </section>
    ),
  },
};

export const Settings: Story = {
  args: {
    inset: "none",
    children: (
      <div className="grid">
        <SurfaceRow>
          <span className="text-ui">Workspace name</span>
          <span className="text-ui text-text-muted">Design studio</span>
        </SurfaceRow>
        <SurfaceRow>
          <ThemePreferenceControl
            layout="row"
            value="system"
            onValueChange={() => undefined}
            label="Appearance"
            lightLabel="Light"
            darkLabel="Dark"
            systemLabel="System"
          />
        </SurfaceRow>
      </div>
    ),
  },
};
