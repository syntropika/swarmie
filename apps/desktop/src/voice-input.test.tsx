import { cleanup, screen, waitFor, act } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
// @vitest-environment happy-dom
import { afterEach, expect, it, vi } from "vitest";

import { renderWithQuery as render } from "~/src/test-fixtures";
import { VoiceInput } from "~/src/voice-input";

class RecognitionMock extends EventTarget {
  static current: RecognitionMock | undefined;
  lang = "";
  continuous = false;
  interimResults = false;
  starts = vi.fn<() => void>();
  stops = vi.fn<() => void>();
  aborts = vi.fn<() => void>();
  constructor() {
    super();
    RecognitionMock.current = this;
  }
  start() {
    this.starts();
  }
  stop() {
    this.stops();
  }
  abort() {
    this.aborts();
  }
}
function currentRecognition() {
  const { current } = RecognitionMock;
  if (current === undefined) {
    throw new Error("Recognition did not start.");
  }
  return current;
}
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
  RecognitionMock.current = undefined;
});
it("shows an unavailable microphone without starting audio in unsupported browsers", async () => {
  vi.stubGlobal("SpeechRecognition", undefined);
  vi.stubGlobal("webkitSpeechRecognition", undefined);
  const text = vi.fn<(...args: unknown[]) => void>();
  render(<VoiceInput onText={text} onError={vi.fn<(...args: unknown[]) => void>()} />);
  const button = screen.getByRole<HTMLButtonElement>("button", {
    name: "Dictate message",
  });
  expect(button.disabled).toBe(true);
  await userEvent.click(button);
  expect(text).not.toHaveBeenCalled();
});
it("starts only on a click and inserts final dictation into the current callback", async () => {
  vi.stubGlobal("SpeechRecognition", RecognitionMock);
  const first = vi.fn<(...args: unknown[]) => void>();
  const latest = vi.fn<(...args: unknown[]) => void>();
  const view = render(
    <VoiceInput onText={first} onError={vi.fn<(...args: unknown[]) => void>()} />,
  );
  await userEvent.click(
    screen.getByRole("button", {
      name: "Dictate message",
    }),
  );
  const recognition = currentRecognition();
  expect(recognition.starts).toHaveBeenCalledOnce();
  view.rerender(<VoiceInput onText={latest} onError={vi.fn<(...args: unknown[]) => void>()} />);
  act(() => {
    recognition.dispatchEvent(
      Object.assign(new Event("result"), {
        resultIndex: 1,
        results: {
          length: 3,
          0: { isFinal: true, 0: { transcript: "Already handled" } },
          1: { isFinal: true, 0: { transcript: "New text" } },
          2: { isFinal: false, 0: { transcript: "Incomplete text" } },
        },
      }),
    );
  });
  expect(first).not.toHaveBeenCalled();
  expect(latest).toHaveBeenLastCalledWith("New text");
  await userEvent.click(
    screen.getByRole("button", {
      name: "Stop dictation",
    }),
  );
  expect(recognition.stops).toHaveBeenCalledOnce();
  act(() => {
    recognition.dispatchEvent(new Event("end"));
  });
  await waitFor(() => {
    expect(
      screen.getByRole("button", {
        name: "Dictate message",
      }),
    ).toBeTruthy();
  });
});
it("reports denied microphone access and detaches callbacks when leaving a conversation", async () => {
  vi.stubGlobal("SpeechRecognition", RecognitionMock);
  const error = vi.fn<(...args: unknown[]) => void>();
  const view = render(
    <VoiceInput onText={vi.fn<(...args: unknown[]) => void>()} onError={error} />,
  );
  await userEvent.click(
    screen.getByRole("button", {
      name: "Dictate message",
    }),
  );
  const recognition = currentRecognition();
  act(() => {
    recognition.dispatchEvent(Object.assign(new Event("error"), { error: "not-allowed" }));
  });
  expect(error).toHaveBeenCalledWith(expect.stringContaining("Microphone access was denied"));
  view.unmount();
  expect(recognition.aborts).toHaveBeenCalledOnce();
  const observed = error.mock.calls.length;
  recognition.dispatchEvent(Object.assign(new Event("error"), { error: "not-allowed" }));
  expect(error).toHaveBeenCalledTimes(observed);
});
