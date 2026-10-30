import { describe, expect, test } from "bun:test";
import { mkdtemp, rm, symlink } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";

import { compare, reportValue, statistics } from "./model";
import type { PerformanceReport } from "./model";
import { budgetFindings, run, selectCases } from "./runner";
import { externalPath, repository, resourceNumber } from "./workers";

function fixture(values: number[] = [10, 10, 10, 10, 10]): PerformanceReport {
  return {
    schema: 1,
    fixtureVersion: 1,
    suite: "quick",
    samples: 5,
    warmups: 1,
    source: {
      revision: "baseline",
      dirty: false,
      worktreeSha256: "source",
      nativeBinarySha256: "binary",
    },
    environment: {
      os: "test",
      arch: "test",
      release: "test",
      cpu: "test",
      logical_cpus: 4,
      memory_bytes: 1024,
      bun: "pinned",
      rust: "pinned",
      git: "pinned",
      node_env: "production",
      build: "release",
      tuning_sha256: "none",
    },
    cases: [
      {
        id: "git.inventory",
        engine: "rust",
        operation: "inventory",
        size: 1000,
        scope: "synthetic fixture",
        metrics: { elapsed_ms: statistics(values), native_commands: statistics([1, 1, 1, 1, 1]) },
        resources: {},
      },
    ],
  };
}

describe("performance comparison", () => {
  test("selects focused work without accepting a misspelled case", () => {
    expect(selectCases("quick", "git.inventory").map((entry) => entry.id)).toEqual([
      "git.inventory",
    ]);
    expect(() => selectCases("quick", "missing")).toThrow();
  });
  test("normalizes actual Bun bigint CPU counters without losing precision", () => {
    expect(resourceNumber(103_839n) / 1000).toBe(103.839);
    expect(resourceNumber(19_726_336)).toBe(19_726_336);
    expect(() => resourceNumber(-1n)).toThrow();
    expect(() => resourceNumber(BigInt(Number.MAX_SAFE_INTEGER) + 1n)).toThrow();
  });
  test("allows source changes and improvements without changing the workload", () => {
    const current = fixture([8, 8, 8, 8, 8]);
    current.source.revision = "candidate";
    expect(compare(fixture(), current).status).toBe("passed");
  });

  test("detects median regression and preserves its evidence", () => {
    const result = compare(fixture(), fixture([16, 16, 16, 16, 16]));
    expect(result.status).toBe("regression");
    expect(result.findings.join(" ")).toContain("median time increased");
  });

  test("does not hide a new native command behind unchanged timing", () => {
    const current = fixture();
    const [entry] = current.cases;
    if (entry === undefined) {
      throw new Error("Missing fixture case.");
    }
    entry.metrics.native_commands = statistics([2, 2, 2, 2, 2]);
    expect(compare(fixture(), current).status).toBe("regression");
  });

  test("rejects unlike hardware, missing cases and changed input sizes", () => {
    const hardware = fixture();
    hardware.environment.cpu = "different";
    expect(compare(fixture(), hardware).status).toBe("incomparable");
    const workload = fixture();
    const [entry] = workload.cases;
    if (entry === undefined) {
      throw new Error("Missing fixture case.");
    }
    entry.size = 5000;
    expect(compare(fixture(), workload).status).toBe("incomparable");
    const missing = fixture();
    missing.cases = [];
    expect(compare(fixture(), missing).status).toBe("incomparable");
    const semantics = fixture();
    const [changed] = semantics.cases;
    if (changed === undefined) {
      throw new Error("Missing fixture case.");
    }
    changed.scope = "a different measurement contract";
    expect(compare(fixture(), semantics).status).toBe("incomparable");
  });

  test("permits small measured RAM drift but refuses a different capacity class", () => {
    const current = fixture();
    current.environment.memory_bytes = 1040;
    expect(compare(fixture(), current).status).toBe("passed");
    current.environment.memory_bytes = 2048;
    expect(compare(fixture(), current).status).toBe("incomparable");
  });

  test("refuses a noisy timing verdict instead of silently passing", () => {
    expect(compare(fixture(), fixture([1, 3, 20, 30, 100])).status).toBe("unstable");
  });

  test("recomputes saved statistics and refuses invalid samples or coverage", () => {
    const saved = fixture();
    const [entry] = saved.cases;
    if (entry?.metrics.elapsed_ms === undefined) {
      throw new Error("Missing fixture metric.");
    }
    entry.metrics.elapsed_ms.median = 0;
    expect(reportValue(saved).cases[0]?.metrics.elapsed_ms?.median).toBe(10);
    entry.metrics.elapsed_ms.samples[0] = Number.NaN;
    expect(() => reportValue(saved)).toThrow();
    expect(() => statistics([])).toThrow();
    expect(() => statistics([Number.POSITIVE_INFINITY])).toThrow();
    const incomplete = fixture();
    incomplete.environment = {};
    expect(() => reportValue(incomplete)).toThrow();
  });

  test("checks absolute work budgets without a timing baseline", () => {
    const current = fixture();
    const [entry] = current.cases;
    if (entry === undefined) {
      throw new Error("Missing fixture case.");
    }
    entry.metrics.native_commands = statistics([201, 201, 201, 201, 201]);
    expect(budgetFindings(current).join(" ")).toContain("exceeds 154");
  });
});

async function rejectsSourceOutput(candidate: string) {
  let failure: unknown;
  try {
    await externalPath(candidate);
  } catch (error: unknown) {
    failure = error;
  }
  expect(failure).toBeInstanceOf(Error);
  expect(failure instanceof Error ? failure.message : "").toContain("outside");
}

test("keeps generated evidence outside source, including symlink aliases", async () => {
  const temporary = await mkdtemp(path.join(tmpdir(), "swarmie-perf-path-"));
  try {
    expect(await externalPath(path.join(temporary, "new", "run"))).toBe(
      path.join(await externalPath(temporary), "new", "run"),
    );
    await rejectsSourceOutput(path.join(repository, "target", "perf"));
    if (process.platform !== "win32") {
      await symlink(repository, path.join(temporary, "alias"));
      await rejectsSourceOutput(path.join(temporary, "alias", "generated"));
    }
  } finally {
    await rm(temporary, { recursive: true, force: true });
  }
});

test("loads an existing baseline before creating output and never self-baselines", async () => {
  const temporary = await mkdtemp(path.join(tmpdir(), "swarmie-perf-baseline-"));
  try {
    const output = path.join(temporary, "new-run");
    let failure: unknown;
    try {
      await run({
        suite: "quick",
        output,
        samples: 5,
        warmups: 1,
        timeoutMs: 1000,
        baseline: path.join(output, "report.json"),
        filter: "git.inventory",
      });
    } catch (error: unknown) {
      failure = error;
    }
    expect(failure).toBeInstanceOf(Error);
    expect(await Bun.file(path.join(output, "report.json")).exists()).toBe(false);
  } finally {
    await rm(temporary, { recursive: true, force: true });
  }
});

test("an explicit comparison prefix isolates cases without hiding missing coverage", () => {
  const before = fixture();
  const after = fixture();
  const [original] = before.cases;
  if (original === undefined) {
    throw new Error("Missing fixture.");
  }
  const noisy = {
    ...original,
    id: "ui.diff",
    metrics: { elapsed_ms: statistics([1, 3, 20, 30, 100]) },
  };
  before.cases.push(noisy);
  after.cases.push(structuredClone(noisy));
  expect(compare(before, after).status).toBe("unstable");
  expect(compare(before, after, "git.").status).toBe("passed");
  expect(compare(before, after, "missing").status).toBe("incomparable");
  after.cases.push({ ...original, id: "git.extra" });
  expect(compare(before, after, "git.").status).toBe("incomparable");
  expect(before.cases.length).toBe(2);
});

test("turn workloads preserve the historical context sizes and separate native worker identity", () => {
  const selected = selectCases("full", "agent.");
  expect(selected).toHaveLength(24);
  expect([...new Set(selected.map((entry) => entry.size))]).toEqual([1, 101, 1001]);
  expect(
    selected.every((entry) => entry.engine === "rust-agent" && entry.operation.endsWith(".60")),
  ).toBe(true);
  const report = fixture();
  const [entry] = selected;
  if (entry === undefined) {
    throw new Error("Missing native turn case.");
  }
  report.cases[0] = {
    ...entry,
    metrics: { elapsed_ms: statistics([1, 1, 1, 1, 1]) },
    resources: {},
  };
  report.source.agentBinarySha256 = "turn-worker";
  expect(reportValue(report).source.agentBinarySha256).toBe("turn-worker");
  expect(reportValue(report).cases[0]?.engine).toBe("rust-agent");
  delete report.source.agentBinarySha256;
  expect(() => reportValue(report)).toThrow("worker identity");
});
