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
