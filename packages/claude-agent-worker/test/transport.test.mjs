import assert from "node:assert/strict";
import { createInterface } from "node:readline";
import { Readable } from "node:stream";
import { test } from "node:test";

import { requestLines, sdkOutputLines } from "../transport.mjs";

test("worker IPC keeps Unicode separators and split UTF-8 inside one JSON record", async () => {
  const message = JSON.stringify({ text: "Hello \u2028 paragraph \u2029 끝" });
  const bytes = Buffer.from(`${message}\n${JSON.stringify({ next: true })}\n`);
  const chunks = Array.from(bytes, (byte) => Buffer.from([byte]));
  const lines = [];
  for await (const line of requestLines(Readable.from(chunks))) {
    lines.push(JSON.parse(line));
  }
  assert.deepEqual(lines, [{ text: "Hello \u2028 paragraph \u2029 끝" }, { next: true }]);
});

test("SDK public process stdout survives Node readline without changing Unicode content", async () => {
  const value = {
    type: "stream_event",
    event: { delta: { text: "Hello \u2028 paragraph \u2029 끝" } },
  };
  const bytes = Buffer.from(`${JSON.stringify(value)}\n`);
  const input = Readable.from(Array.from(bytes, (byte) => Buffer.from([byte]))).pipe(
    sdkOutputLines(),
  );
  const lines = [];
  for await (const line of createInterface({ input })) {
    lines.push(JSON.parse(line));
  }
  assert.deepEqual(lines, [value]);
});

test("worker IPC rejects oversized records", async () => {
  await assert.rejects(async () => {
    for await (const line of requestLines(Readable.from(["long input"]), 4)) {
      void line;
    }
  }, /record limit/u);
  await assert.rejects(async () => {
    for await (const line of requestLines(Readable.from(["long input\n"]), 4)) {
      void line;
    }
  }, /record limit/u);
  await assert.rejects(async () => {
    for await (const line of requestLines(Readable.from([Buffer.from([0xe2])]), 1)) {
      void line;
    }
  }, /record limit/u);
});
