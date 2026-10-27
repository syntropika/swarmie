import type { ComponentProps, PropsWithChildren } from "react";

import { Modal } from "../../atoms/modal";

export function Dialog({
  children,
  open,
  onOpenChange,
}: PropsWithChildren<{ open: boolean; onOpenChange: (open: boolean) => void }>) {
  return (
    <Modal isOpen={open} onOpenChange={onOpenChange}>
      {children}
    </Modal>
  );
}
export function DialogContent({
  children,
  showCloseButton = true,
  ...props
}: Omit<ComponentProps<typeof Modal.Dialog>, "children"> &
  PropsWithChildren & { showCloseButton?: boolean }) {
  return (
    <Modal.Backdrop>
      <Modal.Container>
        <Modal.Dialog {...props}>
          {showCloseButton ? <Modal.CloseTrigger /> : null}
          {children}
        </Modal.Dialog>
      </Modal.Container>
    </Modal.Backdrop>
  );
}
export const DialogTitle = Modal.Heading;
export function DialogDescription(props: ComponentProps<"p">) {
  return <p {...props} />;
}
