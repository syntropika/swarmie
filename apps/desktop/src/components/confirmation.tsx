import { Button, Modal } from "@swarmie/ui-web";
import { useCallback, useEffect, useRef, useState } from "react";

type Confirmation = { scope: string; message: string; accept: () => void };

/** Keep confirmation attached to its view; leaving the view declines pending work. */
export function useConfirmation(scope: string) {
  const [selection, setSelection] = useState<{ scope: string; request?: Confirmation }>({ scope });
  if (selection.scope !== scope) {
    setSelection({ scope });
  }
  const request = selection.scope === scope ? selection.request : undefined;
  const pending = useRef<Confirmation | undefined>(undefined);
  const currentScope = useRef(scope);
  const mounted = useRef(true);
  useEffect(() => {
    currentScope.current = scope;
    mounted.current = true;
    return () => {
      mounted.current = false;

      pending.current = undefined;
    };
  }, [scope]);
  const confirm = useCallback(
    (message: string, accept: () => void) => {
      if (!mounted.current || currentScope.current !== scope) {
        return;
      }
      const next = { scope, message, accept };
      pending.current = next;
      setSelection({ scope, request: next });
    },
    [scope],
  );
  const finish = (confirmed: boolean) => {
    const owned = pending.current;
    pending.current = undefined;
    setSelection({ scope });
    if (confirmed && owned?.scope === scope && mounted.current) {
      owned.accept();
    }
  };
  const dialog =
    request?.scope === scope ? (
      <Modal
        isOpen
        onOpenChange={(next) => {
          if (!next) {
            (() => {
              finish(false);
            })();
          }
        }}
      >
        <Modal.Backdrop isDismissable>
          <Modal.Container>
            <Modal.Dialog aria-label="Confirm action">
              <p>{request.message}</p>
              <div className="flex items-center gap-2">
                <Button
                  variant="tertiary"
                  onClick={() => {
                    finish(false);
                  }}
                >
                  Cancel
                </Button>
                <Button
                  variant="danger"
                  onClick={() => {
                    finish(true);
                  }}
                >
                  Confirm
                </Button>
              </div>
            </Modal.Dialog>
          </Modal.Container>
        </Modal.Backdrop>
      </Modal>
    ) : null;
  return { confirm, dialog };
}
