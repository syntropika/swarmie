import { useState, type ReactNode } from "react";
import * as Primitive from "@radix-ui/react-dialog";
import { cn } from "cn";

export const DialogTitle = Primitive.Title;
export function Dialog({ label, onClose, children, className = "", busy = false }: {
  label: string; onClose: () => void; children: ReactNode; className?: string; busy?: boolean;
}) {
  const [opener] = useState(() => {
    const active = document.activeElement;
    const menu = active?.closest<HTMLElement>("[role=menu]");
    return menu?.id ? document.querySelector<HTMLElement>(`[aria-controls="${menu.id}"]`) : active instanceof HTMLElement ? active : null;
  });
  return <Primitive.Root open onOpenChange={(open) => { if (!open && !busy) onClose(); }}>
    <Primitive.Portal><Primitive.Overlay className="fixed inset-0 z-50 bg-black/50" />
      <div className="pointer-events-none fixed inset-0 z-50 flex items-center justify-center p-4">
        <Primitive.Content aria-label={label} aria-describedby={undefined}
          onCloseAutoFocus={(event) => {
            if (opener?.isConnected) {
              event.preventDefault();
              if (!document.querySelector('[role="dialog"]')) opener.focus();
            }
          }}
          onEscapeKeyDown={(event) => { if (busy) event.preventDefault(); }}
          onInteractOutside={(event) => { if (busy) event.preventDefault(); }}
          className={cn("pointer-events-auto max-h-[calc(100dvh-32px)] w-full max-w-[640px] overflow-y-auto rounded-[20px] bg-[var(--floating-surface)] p-5 text-foreground shadow-[var(--floating-shadow)] outline-none", className)}>
          <Primitive.Title className="sr-only">{label}</Primitive.Title>{children}
        </Primitive.Content>
      </div>
    </Primitive.Portal>
  </Primitive.Root>;
}
