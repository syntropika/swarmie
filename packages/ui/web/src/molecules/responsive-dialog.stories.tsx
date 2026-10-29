import type { Meta, StoryObj } from "@storybook/react-vite";

import { Button } from "../atoms/button";
import { ResponsiveDialog } from "./responsive-dialog";

function Example({
  dismissable = true,
  guided = false,
}: {
  dismissable?: boolean;
  guided?: boolean;
}) {
  return (
    <ResponsiveDialog>
      <Button variant="secondary">Open responsive dialog</Button>
      <ResponsiveDialog.Backdrop isDismissable={dismissable}>
        <ResponsiveDialog.Container size={guided ? "lg" : "md"}>
          <ResponsiveDialog.Dialog {...(guided ? { layout: "guided" } : {})}>
            {guided ? (
              <div className="grid md:grid-cols-[304px_minmax(0,1fr)]">
                <aside className="border-b border-border p-6 md:border-r md:border-b-0">
                  <h2>Example setup</h2>
                  <p>Connect a provider.</p>
                </aside>
                <section className="flex min-w-0 flex-col p-6">
                  <ResponsiveDialog.Header>
                    <ResponsiveDialog.Heading>Choose a provider</ResponsiveDialog.Heading>
                  </ResponsiveDialog.Header>
                  <ResponsiveDialog.Body>
                    <p>This fixture has no connected provider.</p>
                  </ResponsiveDialog.Body>
                  <ResponsiveDialog.Footer>
                    <Button slot="close">Continue</Button>
                  </ResponsiveDialog.Footer>
                </section>
              </div>
            ) : (
              <>
                <ResponsiveDialog.Header>
                  <ResponsiveDialog.Heading>Edit project</ResponsiveDialog.Heading>
                </ResponsiveDialog.Header>
                <ResponsiveDialog.Body>
                  <p>Review project settings before saving.</p>
                </ResponsiveDialog.Body>
                <ResponsiveDialog.Footer>
                  <Button variant="tertiary" slot="close">
                    Cancel
                  </Button>
                  <Button slot="close">Save changes</Button>
                </ResponsiveDialog.Footer>
              </>
            )}
          </ResponsiveDialog.Dialog>
        </ResponsiveDialog.Container>
      </ResponsiveDialog.Backdrop>
    </ResponsiveDialog>
  );
}

const meta = {
  title: "Molecules/Responsive Dialog",
  component: Example,
} satisfies Meta<typeof Example>;
export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};
export const Guided: Story = { args: { guided: true } };
export const NonDismissable: Story = { args: { dismissable: false } };
