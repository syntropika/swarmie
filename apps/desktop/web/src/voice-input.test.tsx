// @vitest-environment happy-dom
import { afterEach, expect, it, vi } from "vitest";
import { cleanup, render, screen, waitFor, act } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { VoiceInput } from "./VoiceInput";

class RecognitionMock {
  static current: RecognitionMock;
  lang = "";
  continuous = false;
  interimResults = false;
  onresult: ((event: unknown) => void) | null = null;
  onerror: ((event: { error: string }) => void) | null = null;
  onend: (() => void) | null = null;
  start = vi.fn();
  stop = vi.fn();
  abort = vi.fn();
  constructor() { RecognitionMock.current = this; }
}
afterEach(() => { cleanup(); vi.unstubAllGlobals(); });

it("shows an unavailable microphone without starting audio in unsupported browsers", async () => {
  vi.stubGlobal("SpeechRecognition", undefined);
  vi.stubGlobal("webkitSpeechRecognition", undefined);
  const text = vi.fn();
  render(<VoiceInput onText={text} onError={vi.fn()} />);
  const button = screen.getByRole("button", { name: "Dictate message" }) as HTMLButtonElement;
  expect(button.disabled).toBe(true);
  await userEvent.click(button);
  expect(text).not.toHaveBeenCalled();
});

it("starts only on a click and inserts final dictation into the current callback", async () => {
  vi.stubGlobal("SpeechRecognition", RecognitionMock);
  const first = vi.fn(), latest = vi.fn();
  const view = render(<VoiceInput onText={first} onError={vi.fn()} />);
  await userEvent.click(screen.getByRole("button", { name: "Dictate message" }));
  const recognition = RecognitionMock.current;
  expect(recognition.start).toHaveBeenCalledOnce();
  view.rerender(<VoiceInput onText={latest} onError={vi.fn()} />);
  act(() => recognition.onresult?.({ resultIndex: 1, results: [
    { isFinal: true, 0: { transcript: "Already handled" } },
    { isFinal: true, 0: { transcript: "New text" } },
    { isFinal: false, 0: { transcript: "Incomplete text" } },
  ] }));
  expect(first).not.toHaveBeenCalled();
  expect(latest).toHaveBeenLastCalledWith("New text");
  await userEvent.click(screen.getByRole("button", { name: "Stop dictation" }));
  expect(recognition.stop).toHaveBeenCalledOnce();
  act(() => recognition.onend?.());
  await waitFor(() => expect(screen.getByRole("button", { name: "Dictate message" })).toBeTruthy());
});

it("reports denied microphone access and detaches callbacks when leaving a conversation", async () => {
  vi.stubGlobal("SpeechRecognition", RecognitionMock);
  const error = vi.fn();
  const view = render(<VoiceInput onText={vi.fn()} onError={error} />);
  await userEvent.click(screen.getByRole("button", { name: "Dictate message" }));
  const recognition = RecognitionMock.current;
  act(() => recognition.onerror?.({ error: "not-allowed" }));
  expect(error).toHaveBeenCalledWith(expect.stringContaining("Microphone access was denied"));
  view.unmount();
  expect(recognition.abort).toHaveBeenCalledOnce();
  expect(recognition.onresult).toBeNull();
});
