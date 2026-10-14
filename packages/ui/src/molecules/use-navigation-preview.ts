import { useCallback, useEffect, useRef } from "react";

type NavigationPreviewOptions = {
  dialog: HTMLDialogElement | null;
  onPreview: (focus: boolean) => void;
  onDismiss: () => void;
};

/** A short crossing grace retains pointer previews while the popup owns keyboard focus. */
export function useNavigationPreview({ dialog, onPreview, onDismiss }: NavigationPreviewOptions) {
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const pointerInside = useRef(false);
  const clearTimer = useCallback(() => {
    clearTimeout(timer.current);
  }, []);
  const scheduleDismissal = useCallback(() => {
    clearTimer();
    timer.current = setTimeout(() => {
      if (!pointerInside.current && dialog?.contains(document.activeElement) !== true) {
        onDismiss();
      }
    }, 120);
  }, [clearTimer, dialog, onDismiss]);
  const enter = useCallback(
    (options: { focus?: boolean } = {}) => {
      clearTimer();
      pointerInside.current = options.focus !== true;
      onPreview(options.focus === true);
    },
    [clearTimer, onPreview],
  );
  const retain = useCallback(() => {
    clearTimer();
    pointerInside.current = true;
  }, [clearTimer]);
  const leave = useCallback(() => {
    pointerInside.current = false;
    scheduleDismissal();
  }, [scheduleDismissal]);
  const cancel = useCallback(() => {
    clearTimer();
    pointerInside.current = false;
  }, [clearTimer]);
  const focusLeave = useCallback(
    (target: EventTarget | null) => {
      if (pointerInside.current || (target instanceof Node && dialog?.contains(target) === true)) {
        return;
      }
      scheduleDismissal();
    },
    [dialog, scheduleDismissal],
  );

  useEffect(() => clearTimer, [clearTimer]);
  return { enter, retain, leave, cancel, focusLeave };
}
