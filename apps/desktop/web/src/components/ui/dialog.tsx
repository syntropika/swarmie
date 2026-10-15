import * as Primitive from "@radix-ui/react-dialog";
import { cva } from "class-variance-authority";
import { cn } from "cn";
import { useRef } from "react";
import type { ReactNode } from "react";

const dialogVariants = cva(
  "text-foreground bg-popover pointer-events-auto max-h-[calc(100dvh-32px)] w-full max-w-[640px] overflow-y-auto rounded-2xl p-5 shadow-xl outline-none",
  {
    variants: {
      variant: {
        default: "",
        onboarding: "harness-onboarding",
        "command-palette": "",
        "context-dialog": "context-dialog",
      },
    },
    defaultVariants: { variant: "default" },
  },
);
export const DialogTitle = Primitive.Title;
export function Dialog({
  label,
  onClose,
  children,
  className = "",
  busy = false,
  variant = "default",
}: {
  label: string;
  onClose: () => void;
  children: ReactNode;
  className?: string;
  busy?: boolean;
  variant?: "default" | "onboarding" | "command-palette" | "context-dialog";
}) {
  const opener = useRef(
    (() => {
      const active = document.activeElement;
      const menu = active?.closest<HTMLElement>("[role=menu]");
      if (menu?.id !== undefined && menu.id.length > 0) {
        return document.querySelector<HTMLElement>(`[aria-controls="${menu.id}"]`);
      }
      return active instanceof HTMLElement ? active : null;
    })(),
  );
  return (
    <Primitive.Root
      open
      onOpenChange={(open) => {
        if (!open && !busy) {
          onClose();
        }
      }}
    >
      <Primitive.Portal>
        <Primitive.Overlay className="fixed inset-0 z-50 bg-black/50" />
        <div className="pointer-events-none fixed inset-0 z-50 flex items-center justify-center p-4">
          <Primitive.Content
            aria-label={label}
            aria-describedby={undefined}
            onCloseAutoFocus={(event) => {
              const target = opener.current;
              if (target?.isConnected === true) {
                event.preventDefault();
                if (!document.querySelector('[role="dialog"]')) {
                  target.focus();
                }
              }
            }}
            onEscapeKeyDown={(event) => {
              if (busy) {
                event.preventDefault();
              }
            }}
            onInteractOutside={(event) => {
              if (busy) {
                event.preventDefault();
              }
            }}
            className={cn(dialogVariants({ variant }), className)}
          >
            <Primitive.Title className="sr-only">{label}</Primitive.Title>
            {children}
          </Primitive.Content>
        </div>
      </Primitive.Portal>
    </Primitive.Root>
  );
}
