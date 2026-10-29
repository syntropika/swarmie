import type { ComponentProps, PropsWithChildren } from "react";

import { ResponsiveDialog } from "../../molecules/responsive-dialog";

export function Dialog({
  children,
  open,
  onOpenChange,
}: PropsWithChildren<{ open: boolean; onOpenChange: (open: boolean) => void }>) {
  return (
    <ResponsiveDialog isOpen={open} onOpenChange={onOpenChange}>
      {children}
    </ResponsiveDialog>
  );
}
export function DialogContent({
  children,
  showCloseButton = true,
  ...props
}: Omit<ComponentProps<typeof ResponsiveDialog.Dialog>, "children"> &
  PropsWithChildren & { showCloseButton?: boolean }) {
  return (
    <ResponsiveDialog.Backdrop>
      <ResponsiveDialog.Container>
        <ResponsiveDialog.Dialog {...props}>
          {showCloseButton ? <ResponsiveDialog.CloseTrigger /> : null}
          {children}
        </ResponsiveDialog.Dialog>
      </ResponsiveDialog.Container>
    </ResponsiveDialog.Backdrop>
  );
}
export const DialogTitle = ResponsiveDialog.Heading;
export function DialogDescription(props: ComponentProps<"p">) {
  return <p {...props} />;
}
