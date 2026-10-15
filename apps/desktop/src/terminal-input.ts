/** Preserve PTY input order across independent asynchronous RPC requests. */
export function createTerminalInput(
  send: (data: string) => Promise<unknown>,
  failed: (error: unknown) => void,
) {
  let active = true;
  let pending = Promise.resolve();
  const rejectInput = (error: unknown) => {
    if (!active) {
      return;
    }
    active = false;
    failed(error);
  };
  return {
    write(data: string) {
      // This queue serializes PTY writes; the rejection handler closes it before notifying its owner.
      if (!active) {
        return;
      }
      pending = pending
        .then(async () => {
          if (active) {
            await send(data);
          }
        })
        .catch(rejectInput);
    },
    dispose() {
      active = false;
    },
  };
}
