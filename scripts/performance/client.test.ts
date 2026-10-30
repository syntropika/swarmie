import { describe, expect, test } from "bun:test";

import { clientOutput, clientSample } from "./client";

const verified = {
  prior_messages: 1000,
  message_bytes: 128,
  warmups: 5,
  model_calls: 1,
  completion_verified: true,
  turn_samples_ns: [1_000_000, 2_000_000],
};

describe("native client timing evidence", () => {
  test("accepts the native test harness prefix but rejects missing or duplicate payloads", () => {
    const line = `test persisted_client_answer_performance ... PERF_CLIENT_TURN ${JSON.stringify(verified)}`;
    expect(clientOutput(`running 1 test\n${line}\nok\n`)).toEqual(verified);
    expect(() => clientOutput("test result: ok")).toThrow();
    expect(() => clientOutput(`${line}\n${line}`)).toThrow();
  });
  test("retains individual verified samples in milliseconds", () => {
    expect(clientSample(verified, 1000, 2)).toEqual([1, 2]);
  });
  test("rejects a different workload or missing terminal evidence", () => {
    expect(() => clientSample(verified, 100, 2)).toThrow();
    expect(() => clientSample({ ...verified, completion_verified: false }, 1000, 2)).toThrow();
    expect(() => clientSample({ ...verified, model_calls: 2 }, 1000, 2)).toThrow();
  });
  test("rejects partial or invalid measurements", () => {
    expect(() => clientSample(verified, 1000, 3)).toThrow();
    expect(() => clientSample({ ...verified, turn_samples_ns: [-1, 2] }, 1000, 2)).toThrow();
  });
});
