import { expect, it, vi } from "vitest";

import { createTerminalInput } from "~/web/src/terminal-input";
import { deferred } from "~/web/src/test-fixtures";

it("waits for each acknowledged terminal write before sending typed or pasted input", async () => {
  const acknowledgement = deferred<undefined>();
  const send = vi
    .fn<(data: string) => Promise<unknown>>()
    .mockImplementationOnce(async () => {
      await acknowledgement.promise;
    })
    .mockResolvedValue(undefined);
  const failed = vi.fn<(error: unknown) => void>();
  const input = createTerminalInput(send, failed);
  input.write("h");
  input.write("e");
  input.write("llo.txt\n");
  await vi.waitFor(() => {
    expect(send).toHaveBeenCalledTimes(1);
  });
  expect(send.mock.calls).toEqual([["h"]]);
  acknowledgement.resolve(undefined);
  await vi.waitFor(() => {
    expect(send).toHaveBeenCalledTimes(3);
  });
  expect(send.mock.calls).toEqual([["h"], ["e"], ["llo.txt\n"]]);
  expect(failed).not.toHaveBeenCalled();
});

it("drops pending input after a failed write or terminal disposal", async () => {
  const error = new Error("Disconnected");
  const send = vi.fn().mockRejectedValue(error);
  const failed = vi.fn<(error: unknown) => void>();
  const input = createTerminalInput(send, failed);
  input.write("command");
  input.write("\n");
  await vi.waitFor(() => {
    expect(failed).toHaveBeenCalledWith(error);
  });
  input.write("later");
  await Promise.resolve();
  expect(send).toHaveBeenCalledTimes(1);
  const disposedSend = vi.fn().mockResolvedValue(undefined);
  const disposed = createTerminalInput(disposedSend, failed);
  disposed.write("queued");
  disposed.dispose();
  await Promise.resolve();
  expect(disposedSend).not.toHaveBeenCalled();
});
