/** Preserve PTY input order across independent asynchronous RPC requests. */
export function createTerminalInput(
  send: (data: string) => Promise<unknown>,
  failed: (error: unknown) => void,
) {
  let active = true;
  let pending = Promise.resolve();
  return {
    write(data: string) {
      if (!active) return;
      pending = pending
        .then(async () => {
          if (active) await send(data);
        })
        .catch((error: unknown) => {
          if (!active) return;
          active = false;
          failed(error);
        });
    },
    dispose() {
      active = false;
    },
  };
}
