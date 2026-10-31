import { expect, test } from "bun:test";

import { metadataFreshness, metadataTime } from "../src/freshness";

test("metadata timestamps retain full u64 values and format only calendar values", () => {
  const value = metadataFreshness({
    prepared_at_ms: "9007199254740993",
    valid_until_ms: "18446744073709551615",
    stale_limit_evidence: 2,
  });
  expect(value.prepared_at_ms).toBe("9007199254740993");
  expect(value.valid_until_ms).toBe("18446744073709551615");
  expect(metadataTime(value.prepared_at_ms)).toBe("Outside the calendar range");
  expect(metadataTime("18446744073709551615")).toBe("Outside the calendar range");
  expect(metadataTime("0")).toBe("1970-01-01T00:00:00.000Z");
  expect(metadataTime("8640000000000000")).toBe("+275760-09-13T00:00:00.000Z");
  expect(metadataTime("8640000000000001")).toBe("Outside the calendar range");
});

test("metadata timestamps reject numeric JSON, overflow and noncanonical strings", () => {
  for (const value of [
    0,
    1,
    Number.MAX_SAFE_INTEGER,
    null,
    undefined,
    true,
    [],
    {},
    "",
    "00",
    "01",
    "-1",
    "+1",
    "1.0",
    "1e3",
    " 1",
    "1 ",
    "1\n",
    "١",
    "18446744073709551616",
    "100000000000000000000",
  ]) {
    expect(() =>
      metadataFreshness({
        prepared_at_ms: value,
        valid_until_ms: null,
        stale_limit_evidence: 0,
      }),
    ).toThrow();
    if (value !== null) {
      expect(() =>
        metadataFreshness({
          prepared_at_ms: "0",
          valid_until_ms: value,
          stale_limit_evidence: 0,
        }),
      ).toThrow();
    }
  }
});

test("reuse boundary and stale evidence remain separate from request age", () => {
  expect(
    metadataFreshness({
      prepared_at_ms: "100",
      valid_until_ms: null,
      stale_limit_evidence: 1,
    }),
  ).toEqual({ prepared_at_ms: "100", valid_until_ms: null, stale_limit_evidence: 1 });
  for (const until of ["99", "100"]) {
    expect(() =>
      metadataFreshness({
        prepared_at_ms: "100",
        valid_until_ms: until,
        stale_limit_evidence: 0,
      }),
    ).toThrow();
  }
  for (const stale of [-1, 0.5, 4_294_967_296, "1"]) {
    expect(() =>
      metadataFreshness({
        prepared_at_ms: "100",
        valid_until_ms: "200",
        stale_limit_evidence: stale,
      }),
    ).toThrow();
  }
});
