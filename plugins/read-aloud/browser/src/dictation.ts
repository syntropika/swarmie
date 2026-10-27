/** The browser recognition subset used here, including engines with the webkit prefix. */
export type RecognitionResult = {
  readonly isFinal: boolean;
  readonly 0: { readonly transcript: string };
};
export type RecognitionResults = {
  readonly resultIndex: number;
  readonly results: ArrayLike<RecognitionResult>;
};
export type RecognitionError = { readonly error: string };
export type RecognitionEvents = {
  start: undefined;
  end: undefined;
  error: RecognitionError;
  result: RecognitionResults;
};
export type RecognitionListener = {
  [K in keyof RecognitionEvents]: [type: K, listener: (event: RecognitionEvents[K]) => void];
}[keyof RecognitionEvents];
export type Recognition = {
  addEventListener: (...args: RecognitionListener) => void;
  removeEventListener: (...args: RecognitionListener) => void;
  lang: string;
  continuous: boolean;
  interimResults: boolean;
  start: () => void;
  stop: () => void;
  abort: () => void;
};
export type RecognitionConstructor = new () => Recognition;
export type RecognitionEnvironment = {
  readonly SpeechRecognition?: RecognitionConstructor;
  readonly webkitSpeechRecognition?: RecognitionConstructor;
};
export function recognitionConstructor(): RecognitionConstructor | undefined {
  const browser: typeof globalThis & RecognitionEnvironment = globalThis;
  return browser.SpeechRecognition ?? browser.webkitSpeechRecognition;
}

/** Commit only final results once; disposal fences late callbacks before aborting capture. */
export function startDictation(
  Constructor: RecognitionConstructor,
  options: {
    readonly language: string;
    readonly insertText: (text: string) => void;
    readonly onListening: (listening: boolean) => void;
    readonly onError: (error: string) => void;
  },
) {
  const recognition = new Constructor();
  let disposed = false;
  const committed = new Set<number>();
  recognition.lang = options.language;
  recognition.continuous = true;
  recognition.interimResults = false;
  function onStart() {
    if (!disposed) {
      options.onListening(true);
    }
  }
  function onEnd() {
    if (!disposed) {
      options.onListening(false);
    }
  }
  function onError(error: RecognitionError) {
    if (!disposed) {
      options.onListening(false);
      options.onError(`Dictation failed: ${error.error}.`);
    }
  }
  function onResult(resultEvent: RecognitionResults) {
    if (disposed) {
      return;
    }
    const parts: string[] = [];
    for (let index = resultEvent.resultIndex; index < resultEvent.results.length; index += 1) {
      const result = resultEvent.results[index];
      if (result?.isFinal === true && !committed.has(index)) {
        committed.add(index);
        parts.push(result[0].transcript.trim());
      }
    }
    const text = parts.filter(Boolean).join(" ");
    if (text !== "") {
      options.insertText(text);
    }
  }
  recognition.addEventListener("start", onStart);
  recognition.addEventListener("end", onEnd);
  recognition.addEventListener("error", onError);
  recognition.addEventListener("result", onResult);
  function dispose() {
    disposed = true;
    recognition.removeEventListener("start", onStart);
    recognition.removeEventListener("end", onEnd);
    recognition.removeEventListener("error", onError);
    recognition.removeEventListener("result", onResult);
    recognition.abort();
  }
  try {
    recognition.start();
  } catch (error: unknown) {
    dispose();
    throw error;
  }
  return {
    stop() {
      recognition.stop();
    },
    dispose,
  };
}
