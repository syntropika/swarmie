// Adapted from the HeroUI Apache-2.0 documentation example: https://raw.githubusercontent.com/heroui-inc/heroui/7a6cbbd6f6622f2562d26349f4728033b626706b/apps/docs/src/demos/en/drawer/basic.tsx
import { Button, Drawer } from "../../src/index";

export function OfficialExample() {
  return (
    <Drawer>
      <Button variant="secondary">Open Drawer</Button>
      <Drawer.Backdrop>
        <Drawer.Content placement="right">
          <Drawer.Dialog>
            <Drawer.Header>
              <Drawer.Heading>Drawer Title</Drawer.Heading>
            </Drawer.Header>
            <Drawer.Body>
              <p>
                This is a bottom drawer built with React Aria&apos;s Modal component. It slides up
                from the bottom of the screen with a smooth CSS transition.
              </p>
            </Drawer.Body>
            <Drawer.Footer>
              <Button slot="close" variant="secondary">
                Cancel
              </Button>
              <Button slot="close">Confirm</Button>
            </Drawer.Footer>
          </Drawer.Dialog>
        </Drawer.Content>
      </Drawer.Backdrop>
    </Drawer>
  );
}
