import { Modal as HeroModal } from "@heroui/react";
import type {
  Modal as HeroModalTypes,
  ModalDialogProps as HeroModalDialogProps,
  ModalHeadingProps as HeroModalHeadingProps,
} from "@heroui/react";
import type { ComponentProps } from "react";

import { cn } from "../classes";

export type ModalDialogProps = HeroModalDialogProps & {
  /** Edge-to-edge content with independently composed navigation and body columns. */
  layout?: "default" | "split" | "guided";
};

function Dialog({ layout = "default", className, ...props }: ModalDialogProps) {
  return (
    <HeroModal.Dialog
      {...props}
      data-layout={layout}
      className={cn(
        layout !== "default" &&
          "w-[min(1000px,calc(100vw-64px))] max-w-none rounded-panel border border-border p-0",
        layout === "guided" && "bg-surface",
        className,
      )}
    />
  );
}

export type ModalHeadingProps = HeroModalHeadingProps & {
  /** Guided flows have a stronger title hierarchy than compact dialogs. */
  scale?: "default" | "title";
};

function Heading({ scale = "default", className, ...props }: ModalHeadingProps) {
  return (
    <HeroModal.Heading
      {...props}
      className={cn(scale === "title" && "text-title font-medium tracking-tight", className)}
    />
  );
}

function Root(props: ComponentProps<typeof HeroModal>) {
  return <HeroModal {...props} />;
}

// oxlint-disable-next-line no-redeclare -- Preserve the compound vendor value and prop namespace.
export const Modal = Object.assign(Root, HeroModal, { Dialog, Heading });
export type Modal = Omit<HeroModalTypes, "DialogProps" | "HeadingProps"> & {
  DialogProps: ModalDialogProps;
  HeadingProps: ModalHeadingProps;
};
export type { ModalProps } from "@heroui/react";
