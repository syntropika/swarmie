import { expect, test } from "bun:test";

import { startDictation } from "../src/dictation";
import type {
  Recognition,
  RecognitionResult,
  RecognitionListener,
  RecognitionEvents,
} from "../src/dictation";

function fixture() {
  class BrowserRecognition implements Recognition {
    static readonly instances: BrowserRecognition[] = [];
    readonly listeners = {
      start: new Set<(event: undefined) => void>(),
      end: new Set<(event: undefined) => void>(),
      error: new Set<(event: RecognitionEvents["error"]) => void>(),
      result: new Set<(event: RecognitionEvents["result"]) => void>(),
    };
    lang = "";
    continuous = false;
    interimResults = true;
    stopped = false;
    aborted = false;
    addEventListener(...args: RecognitionListener) {
      this.updateListener(true, ...args);
    }
    removeEventListener(...args: RecognitionListener) {
      this.updateListener(false, ...args);
    }
    updateListener(add: boolean, ...args: RecognitionListener) {
      const method = add ? "add" : "delete";
      if (args[0] === "start") {
        this.listeners.start[method](args[1]);
      } else if (args[0] === "end") {
        this.listeners.end[method](args[1]);
      } else if (args[0] === "error") {
        this.listeners.error[method](args[1]);
      } else {
        this.listeners.result[method](args[1]);
      }
    }
    start() {
      BrowserRecognition.instances.push(this);
      for (const listener of this.listeners.start) {
        listener(undefined);
      }
    }
    stop() {
      this.stopped = true;
    }
    abort() {
      this.aborted = true;
    }
    results(results: readonly RecognitionResult[], resultIndex = 0) {
      for (const listener of this.listeners.result) {
        listener({ results, resultIndex });
      }
    }
    error(error: string) {
      for (const listener of this.listeners.error) {
        listener({ error });
      }
    }
    end() {
      for (const listener of this.listeners.end) {
        listener(undefined);
      }
    }
  }
  const text: string[] = [];
  const states: boolean[] = [];
  const errors: string[] = [];
  const session = startDictation(BrowserRecognition, {
    language: "es-ES",
    insertText(value) {
      text.push(value);
    },
    onListening(value) {
      states.push(value);
    },
    onError(error) {
      errors.push(error);
    },
  });
  const [browser] = BrowserRecognition.instances;
  if (browser === undefined) {
    throw new Error("Recognition did not start");
  }
  return { session, text, states, errors, browser };
}
const result = (transcript: string, isFinal = true): RecognitionResult => ({
  0: { transcript },
  isFinal,
});
test("dictation commits final results once and leaves interim text outside the draft", () => {
  const { browser, text, session } = fixture();
  expect(browser.lang).toBe("es-ES");
  expect(browser.continuous).toBe(true);
  expect(browser.interimResults).toBe(false);
  browser.results([result("provisional", false)]);
  expect(text).toEqual([]);
  const results = [result("  Hello  "), result("world")];
  browser.results(results);
  browser.results(results);
  browser.results([...results, result("again")], 2);
  expect(text).toEqual(["Hello world", "again"]);
  session.stop();
  expect(browser.stopped).toBe(true);
  expect(browser.aborted).toBe(false);
});
test("retiring dictation aborts capture and removes late event listeners", () => {
  const { browser, session, text, states, errors } = fixture();
  session.dispose();
  browser.results([result("wrong conversation")]);
  browser.error("not-allowed");
  browser.end();
  expect(browser.aborted).toBe(true);
  expect(text).toEqual([]);
  expect(errors).toEqual([]);
  expect(states).toEqual([true]);
});
test("recognition reports permission failure and does not restart automatically", () => {
  const { browser, states, errors } = fixture();
  browser.error("not-allowed");
  browser.end();
  expect(states).toEqual([true, false, false]);
  expect(errors).toEqual(["Dictation failed: not-allowed."]);
});
