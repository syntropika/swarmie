import { Drawer as HeroDrawer, Modal as HeroModal } from "@heroui/react";
import type {
  Modal as HeroModalTypes,
  ModalDialogProps as HeroModalDialogProps,
} from "@heroui/react";
import type { ComponentProps } from "react";
import { createContext, useContext, useState } from "react";

import { cn } from "../classes";
import { usePhoneLayout } from "../use-phone-layout";

const SheetContext = createContext(false);
const DismissableContext = createContext(true);
export type ResponsiveDialogDialogProps = HeroModalDialogProps & {
  /** Edge-to-edge content with independently composed navigation and body columns. */
  layout?: "default" | "split" | "guided";
};

function Dialog({
  layout = "default",
  className,
  children,
  ...props
}: ResponsiveDialogDialogProps) {
  const sheet = useContext(SheetContext);
  const dismissable = useContext(DismissableContext);
  const Component = sheet ? HeroDrawer.Dialog : HeroModal.Dialog;
  return (
    <Component
      {...props}
      data-layout={layout}
      data-swarmie-modal=""
      className={cn(
        layout !== "default" && "p-0",
        !sheet && layout !== "default" && "w-[min(1000px,calc(100vw-64px))] max-w-none",
        sheet &&
          "max-h-[calc(var(--visual-viewport-height,100dvh)-env(safe-area-inset-top,0px)-var(--spacing-control-touch))] w-full max-w-none touch-pan-y! overflow-hidden [&>form]:min-h-0 [&>form]:overflow-y-auto [&>form]:overscroll-contain",
        className,
      )}
    >
      {(state) => (
        <>
          {sheet && dismissable ? <HeroDrawer.Handle className="touch-none" /> : null}
          {typeof children === "function" ? children(state) : children}
        </>
      )}
    </Component>
  );
}

function Heading(props: ComponentProps<typeof HeroModal.Heading>) {
  const Component = useContext(SheetContext) ? HeroDrawer.Heading : HeroModal.Heading;
  return <Component {...props} />;
}

export type ResponsiveDialogProps = ComponentProps<typeof HeroModal>;

function Root(props: ResponsiveDialogProps) {
  const phone = usePhoneLayout();
  const [observedOpen, setObservedOpen] = useState(props.defaultOpen ?? false);
  const open = props.state?.isOpen ?? props.isOpen ?? observedOpen;
  const [sheet, setSheet] = useState(phone);
  // Keep the active presentation stable while the user edits a dialog.
  if (!open && sheet !== phone) {
    setSheet(phone);
  }
  const Component = sheet ? HeroDrawer : HeroModal;
  return (
    <SheetContext value={sheet}>
      <Component
        {...props}
        onOpenChange={(next) => {
          setObservedOpen(next);
          props.onOpenChange?.(next);
        }}
      />
    </SheetContext>
  );
}

function Backdrop(props: ComponentProps<typeof HeroModal.Backdrop>) {
  const Component = useContext(SheetContext) ? HeroDrawer.Backdrop : HeroModal.Backdrop;
  return (
    <DismissableContext value={props.isDismissable !== false}>
      <Component {...props} />
    </DismissableContext>
  );
}

function Container({
  placement,
  scroll,
  size,
  ...props
}: ComponentProps<typeof HeroModal.Container>) {
  if (useContext(SheetContext)) {
    return <HeroDrawer.Content {...props} placement="bottom" />;
  }
  return (
    <HeroModal.Container
      {...props}
      {...(placement === undefined ? {} : { placement })}
      {...(scroll === undefined ? {} : { scroll })}
      {...(size === undefined ? {} : { size })}
    />
  );
}

function Header({ className, ...props }: ComponentProps<typeof HeroModal.Header>) {
  const sheet = useContext(SheetContext);
  const Component = sheet ? HeroDrawer.Header : HeroModal.Header;
  return <Component {...props} className={cn(sheet && "touch-none", className)} />;
}

function Body(props: ComponentProps<typeof HeroModal.Body>) {
  const Component = useContext(SheetContext) ? HeroDrawer.Body : HeroModal.Body;
  return <Component {...props} />;
}

function Footer(props: ComponentProps<typeof HeroModal.Footer>) {
  const Component = useContext(SheetContext) ? HeroDrawer.Footer : HeroModal.Footer;
  return <Component {...props} />;
}

function CloseTrigger(props: ComponentProps<typeof HeroModal.CloseTrigger>) {
  const Component = useContext(SheetContext) ? HeroDrawer.CloseTrigger : HeroModal.CloseTrigger;
  return <Component {...props} />;
}

function Icon(props: ComponentProps<typeof HeroModal.Icon>) {
  return <HeroModal.Icon {...props} />;
}

function Trigger(props: ComponentProps<typeof HeroModal.Trigger>) {
  return <HeroModal.Trigger {...props} />;
}

// oxlint-disable-next-line no-redeclare -- Preserve the compound vendor value and prop namespace.
export const ResponsiveDialog = Object.assign(Root, HeroModal, {
  Root,
  Backdrop,
  Container,
  Dialog,
  Header,
  Body,
  Footer,
  Heading,
  CloseTrigger,
  Icon,
  Trigger,
});
export type ResponsiveDialog = Omit<HeroModalTypes, "DialogProps"> & {
  DialogProps: ResponsiveDialogDialogProps;
};
