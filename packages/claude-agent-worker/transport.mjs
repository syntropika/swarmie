import { Duplex } from "node:stream";
import { StringDecoder } from "node:string_decoder";

function normalize(text) {
  return text.replaceAll("\u2028", "\\u2028").replaceAll("\u2029", "\\u2029");
}
/** Protects SDK output parsing from Unicode separators inside JSON strings. */
export function sdkOutputLines() {
  return Duplex.from(async function* normalizedOutput(input) {
    const decoder = new StringDecoder("utf-8");
    for await (const chunk of input) {
      yield normalize(decoder.write(chunk));
    }
    yield normalize(decoder.end());
  });
}
/**
 * IPC records end only at LF; user content may contain literal Unicode separators.
 * @yields {string} One complete JSON record, without the trailing LF.
 */
export async function* requestLines(input, maxBytes = 8 * 1024 * 1024) {
  const decoder = new StringDecoder("utf-8");
  let pending = "";
  for await (const chunk of input) {
    pending += typeof chunk === "string" ? chunk : decoder.write(chunk);
    let end;
    while ((end = pending.indexOf("\n")) !== -1) {
      const line = pending.slice(0, end);
      pending = pending.slice(end + 1);
      if (Buffer.byteLength(line) > maxBytes) {
        throw new Error("Claude SDK worker input exceeds its record limit");
      }
      if (line.trim()) {
        yield line;
      }
    }
    if (Buffer.byteLength(pending) > maxBytes) {
      throw new Error("Claude SDK worker input exceeds its record limit");
    }
  }
  pending += decoder.end();
  if (Buffer.byteLength(pending) > maxBytes) {
    throw new Error("Claude SDK worker input exceeds its record limit");
  }
  if (pending.trim()) {
    yield pending;
  }
}
