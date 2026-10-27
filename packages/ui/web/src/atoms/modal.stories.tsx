import type { Meta, StoryObj } from "@storybook/react-vite";

import { officialExampleSource } from "../../.storybook/example-sources";
import { OfficialExample as HeroUIDocumentationExample } from "../../.storybook/examples/modal";
import { Button } from "./button";
import { Modal } from "./modal";

const meta = { title: "Atoms/Modal", component: Modal, args: { children: null } } satisfies Meta<
  typeof Modal
>;
export default meta;
type Story = StoryObj<typeof meta>;
export const Default: Story = {
  render: () => (
    <Modal>
      <Button variant="secondary">Open modal</Button>
      <Modal.Backdrop>
        <Modal.Container>
          <Modal.Dialog>
            <Modal.Header>
              <Modal.Heading>Edit project</Modal.Heading>
            </Modal.Header>
            <Modal.Body>
              <p>Review project settings before saving.</p>
            </Modal.Body>
            <Modal.Footer>
              <Button variant="tertiary" slot="close">
                Cancel
              </Button>
              <Button slot="close">Save changes</Button>
            </Modal.Footer>
          </Modal.Dialog>
        </Modal.Container>
      </Modal.Backdrop>
    </Modal>
  ),
};

export const OfficialExample: Story = {
  parameters: { docs: { source: { code: officialExampleSource("modal"), language: "tsx" } } },
  name: "Official example",
  render: () => <HeroUIDocumentationExample />,
};

export const SplitLayout: Story = {
  render: () => (
    <Modal>
      <Button variant="secondary">Open guided setup</Button>
      <Modal.Backdrop>
        <Modal.Container size="lg" placement="center">
          <Modal.Dialog layout="guided" aria-label="Guided setup">
            <Modal.CloseTrigger />
            <div className="grid min-h-[400px] grid-cols-[304px_minmax(0,1fr)]">
              <aside className="flex flex-col gap-4 border-r border-border p-6">
                <h2 className="text-title font-medium">Get started</h2>
                <p className="text-caption text-muted">Connect a provider</p>
              </aside>
              <section className="flex flex-col gap-6 p-6">
                <Modal.Header>
                  <Modal.Heading scale="title">Choose a provider</Modal.Heading>
                </Modal.Header>
                <Modal.Body>
                  <p>Configure a connection before choosing a model.</p>
                </Modal.Body>
                <Modal.Footer>
                  <Button slot="close">Continue</Button>
                </Modal.Footer>
              </section>
            </div>
          </Modal.Dialog>
        </Modal.Container>
      </Modal.Backdrop>
    </Modal>
  ),
};
