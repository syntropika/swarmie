import { isRecord } from "~/src/checked-json";
import { hasText } from "~/src/value-presence";

type Recognition = EventTarget & {
  lang: string;
  continuous: boolean;
  interimResults: boolean;
  start: () => void;
  stop: () => void;
  abort: () => void;
};
type Constructor = new () => Recognition;
function isConstructor(value: unknown): value is Constructor {
  if (typeof value !== "function") {
    return false;
  }
  const prototype: unknown = Reflect.get(value, "prototype");
  return (
    isRecord(prototype) &&
    ["start", "stop", "abort", "addEventListener", "removeEventListener"].every(
      (method) => typeof Reflect.get(prototype, method) === "function",
    )
  );
}
export function recognitionConstructor(): Constructor | undefined {
  for (const name of ["SpeechRecognition", "webkitSpeechRecognition"]) {
    const candidate: unknown = Reflect.get(window, name);
    if (isConstructor(candidate)) {
      return candidate;
    }
  }
  return undefined;
}
function transcript(event: Event): string {
  const index: unknown = Reflect.get(event, "resultIndex");
  const results: unknown = Reflect.get(event, "results");
  if (typeof index !== "number" || !isRecord(results) || typeof results.length !== "number") {
    return "";
  }
  const parts: string[] = [];
  for (let current = index; current < results.length; current += 1) {
    const result: unknown = Reflect.get(results, String(current));
    if (!isRecord(result) || result.isFinal !== true) {
      continue;
    }
    const first: unknown = Reflect.get(result, "0");
    if (isRecord(first) && typeof first.transcript === "string") {
      parts.push(first.transcript);
    }
  }
  return parts.join(" ").trim();
}
export function startBrowserSpeech(options: {
  language: string;
  text: (value: string) => void;
  error: (value: string) => void;
  ended: () => void;
}) {
  const Constructor = recognitionConstructor();
  if (Constructor === undefined) {
    throw new Error("Voice dictation is unavailable in this browser.");
  }
  const recognition = new Constructor();
  recognition.lang = options.language;
  recognition.continuous = true;
  recognition.interimResults = false;
  const result = (event: Event) => {
    const text = transcript(event);
    if (hasText(text)) {
      options.text(text);
    }
  };
  const speechError = (event: Event) => {
    const code: unknown = Reflect.get(event, "error");
    if (code === "aborted" || code === "no-speech") {
      return;
    }
    options.error(
      code === "not-allowed"
        ? "Microphone access was denied. Allow microphone access and try again."
        : "Voice dictation failed. Try again or type your message.",
    );
  };
  const detach = () => {
    recognition.removeEventListener("result", result);
    recognition.removeEventListener("error", speechError);
    recognition.removeEventListener("end", ended);
  };
  function ended() {
    detach();
    options.ended();
  }
  recognition.addEventListener("result", result);
  recognition.addEventListener("error", speechError);
  recognition.addEventListener("end", ended);
  try {
    recognition.start();
  } catch (error) {
    detach();
    throw error;
  }
  return {
    stop: () => {
      recognition.stop();
    },
    abort: () => {
      detach();
      recognition.abort();
    },
  };
}
