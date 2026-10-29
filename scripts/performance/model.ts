import type { PerformanceCase } from "./cases";

export type Statistics = {
  samples: number[];
  min: number;
  median: number;
  p95: number;
  max: number;
  cv: number;
};
export type CaseResult = PerformanceCase & {
  metrics: Record<string, Statistics>;
  resources: Record<string, Statistics>;
};
export type PerformanceReport = {
  schema: 1;
  fixtureVersion: number;
  suite: "quick" | "full";
  samples: number;
  warmups: number;
  source: { revision: string; dirty: boolean; worktreeSha256: string; nativeBinarySha256: string };
  environment: Record<string, string | number>;
  cases: CaseResult[];
};

export function statistics(samples: readonly number[]): Statistics {
  if (samples.length === 0 || samples.some((value) => !Number.isFinite(value) || value < 0)) {
    throw new Error("Performance samples must be finite, nonnegative and nonempty.");
  }
  const sorted = samples.toSorted((left, right) => left - right);
  const middle = Math.floor(sorted.length / 2);
  const mean = sorted.reduce((sum, value) => sum + value, 0) / sorted.length;
  const variance = sorted.reduce((sum, value) => sum + (value - mean) ** 2, 0) / sorted.length;
  if (!Number.isFinite(mean) || !Number.isFinite(variance)) {
    throw new TypeError("Performance samples exceed the numeric range.");
  }
  return {
    samples: [...samples],
    min: sorted[0] ?? 0,
    median:
      sorted.length % 2
        ? (sorted[middle] ?? 0)
        : ((sorted[middle - 1] ?? 0) + (sorted[middle] ?? 0)) / 2,
    p95: sorted[Math.ceil(sorted.length * 0.95) - 1] ?? 0,
    max: sorted.at(-1) ?? 0,
    cv: mean === 0 ? 0 : Math.sqrt(variance) / mean,
  };
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

export function record(value: unknown): Record<string, unknown> {
  if (!isRecord(value)) {
    throw new TypeError("Expected a performance object.");
  }
  return value;
}

export function finite(value: unknown): number {
  if (typeof value !== "number" || !Number.isFinite(value) || value < 0) {
    throw new TypeError("Expected a finite nonnegative performance value.");
  }
  return value;
}

function integer(value: unknown, minimum = 0): number {
  const number = finite(value);
  if (!Number.isSafeInteger(number) || number < minimum) {
    throw new TypeError("Invalid performance sample count.");
  }
  return number;
}

function text(value: unknown): string {
  if (typeof value !== "string" || value.length === 0) {
    throw new TypeError("Expected a performance identifier.");
  }
  return value;
}

function measurements(value: unknown, sampleCount: number): Record<string, Statistics> {
  return Object.fromEntries(
    Object.entries(record(value)).map(([key, item]) => {
      const { samples } = record(item);
      if (!Array.isArray(samples) || samples.length !== sampleCount) {
        throw new TypeError("Performance sample counts do not match.");
      }
      return [key, statistics(samples.map(finite))];
    }),
  );
}

/** Recompute summaries from raw samples. Never trust a saved percentile alone. */
export function reportValue(value: unknown): PerformanceReport {
  const input = record(value);
  if (input.schema !== 1 || (input.suite !== "quick" && input.suite !== "full")) {
    throw new TypeError("Unsupported performance report.");
  }
  const source = record(input.source);
  if (
    typeof source.dirty !== "boolean" ||
    !Array.isArray(input.cases) ||
    input.cases.length === 0
  ) {
    throw new TypeError("Incomplete performance report.");
  }
  const samples = integer(input.samples, 1);
  const environment = Object.fromEntries(
    Object.entries(record(input.environment)).map(([key, item]) => [
      key,
      typeof item === "string" ? text(item) : finite(item),
    ]),
  );
  if (
    [
      "os",
      "arch",
      "release",
      "cpu",
      "logical_cpus",
      "memory_bytes",
      "bun",
      "rust",
      "git",
      "node_env",
      "build",
      "tuning_sha256",
    ].some((key) => environment[key] === undefined)
  ) {
    throw new TypeError("Incomplete measurement environment.");
  }
  const entries = input.cases.map((item): CaseResult => {
    const entry = record(item);
    if (entry.engine !== "rust" && entry.engine !== "bun") {
      throw new TypeError("Unknown performance engine.");
    }
    const metrics = measurements(entry.metrics, samples);
    if (metrics.elapsed_ms === undefined) {
      throw new TypeError("A performance case has no elapsed-time samples.");
    }
    return {
      id: text(entry.id),
      engine: entry.engine,
      operation: text(entry.operation),
      size: integer(entry.size, 1),
      scope: text(entry.scope),
      metrics,
      resources: measurements(entry.resources, samples),
    };
  });
  if (new Set(entries.map((entry) => entry.id)).size !== entries.length) {
    throw new TypeError("Duplicate performance case.");
  }
  return {
    schema: 1,
    fixtureVersion: integer(input.fixtureVersion, 1),
    suite: input.suite,
    samples,
    warmups: integer(input.warmups),
    source: {
      revision: text(source.revision),
      dirty: source.dirty,
      worktreeSha256: text(source.worktreeSha256),
      nativeBinarySha256: text(source.nativeBinarySha256),
    },
    environment,
    cases: entries,
  };
}

export type Comparison = {
  status: "passed" | "regression" | "incomparable" | "unstable";
  findings: string[];
};

function ordered(value: Record<string, string | number>): string {
  return JSON.stringify(
    Object.entries(value)
      .filter(([key]) => key !== "memory_bytes")
      .toSorted(([left], [right]) => left.localeCompare(right)),
  );
}

function compatibleMemory(left: PerformanceReport, right: PerformanceReport): boolean {
  const before = left.environment.memory_bytes;
  const after = right.environment.memory_bytes;
  // Small capacity changes occur with virtual-machine memory ballooning.
  return (
    typeof before === "number" &&
    typeof after === "number" &&
    before > 0 &&
    after > 0 &&
    Math.abs(after - before) <= Math.min(before, after) * 0.05
  );
}

function metricFindings(
  name: string,
  before: Record<string, Statistics>,
  after: Record<string, Statistics>,
): string[] {
  if (
    JSON.stringify(Object.keys(before).toSorted()) !== JSON.stringify(Object.keys(after).toSorted())
  ) {
    throw new TypeError(`${name}: metric coverage changed.`);
  }
  const findings: string[] = [];
  for (const [key, baseline] of Object.entries(before)) {
    const current = after[key];
    if (current === undefined) {
      throw new TypeError(`${name}: missing metric ${key}.`);
    }
    if (key === "elapsed_ms") {
      if (current.median > baseline.median * 1.2 + 2) {
        findings.push(
          `${name}: median time increased from ${baseline.median.toFixed(2)} to ${current.median.toFixed(2)} ms.`,
        );
      }
      if (current.p95 > baseline.p95 * 1.3 + 2) {
        findings.push(
          `${name}: p95 time increased from ${baseline.p95.toFixed(2)} to ${current.p95.toFixed(2)} ms.`,
        );
      }
    } else if (
      ["native_commands", "rendered_rows", "html_bytes"].includes(key) &&
      current.max > baseline.max
    ) {
      findings.push(`${name}: ${key} increased from ${baseline.max} to ${current.max}.`);
    } else if (
      key === "worker_peak_rss_bytes" &&
      current.median > baseline.median * 1.2 + 4 * 1024 * 1024
    ) {
      findings.push(`${name}: worker peak RSS increased by more than 20% plus 4 MiB.`);
    }
  }
  return findings;
}

export function compare(
  baseline: PerformanceReport,
  current: PerformanceReport,
  filter: string | null = null,
): Comparison {
  if (filter !== null) {
    const before = baseline.cases.filter((entry) => entry.id.startsWith(filter));
    const after = current.cases.filter((entry) => entry.id.startsWith(filter));
    if (before.length === 0 || after.length === 0) {
      return {
        status: "incomparable",
        findings: ["The selected case prefix is absent from one report."],
      };
    }
    return compare({ ...baseline, cases: before }, { ...current, cases: after });
  }
  const shape = (report: PerformanceReport) =>
    JSON.stringify(
      report.cases
        .map(({ id, engine, operation, size, scope }) => ({ id, engine, operation, size, scope }))
        .toSorted((left, right) => left.id.localeCompare(right.id)),
    );
  if (
    baseline.fixtureVersion !== current.fixtureVersion ||
    baseline.suite !== current.suite ||
    baseline.warmups !== current.warmups ||
    baseline.samples !== current.samples ||
    ordered(baseline.environment) !== ordered(current.environment) ||
    !compatibleMemory(baseline, current) ||
    shape(baseline) !== shape(current)
  ) {
    return {
      status: "incomparable",
      findings: [
        "Measurement environments, sample policy or workload coverage differ. Establish an explicit compatible baseline.",
      ],
    };
  }
  const findings: string[] = [];
  try {
    for (const entry of current.cases) {
      const before = baseline.cases.find((candidate) => candidate.id === entry.id);
      if (before === undefined) {
        throw new TypeError("Missing baseline case.");
      }
      findings.push(
        ...metricFindings(entry.id, before.metrics, entry.metrics),
        ...metricFindings(entry.id, before.resources, entry.resources),
      );
    }
  } catch (error: unknown) {
    return {
      status: "incomparable",
      findings: [error instanceof Error ? error.message : String(error)],
    };
  }
  const noisy = [...baseline.cases, ...current.cases]
    .filter((entry) => (entry.metrics.elapsed_ms?.cv ?? 1) > 0.2)
    .map((entry) => entry.id);
  if (baseline.samples < 5 || noisy.length > 0) {
    return {
      status: "unstable",
      findings: [
        ...findings,
        `Timing requires at least five samples and variation of at most 20%. Noisy cases: ${[...new Set(noisy)].join(", ") || "insufficient samples"}. Work-counter findings still apply; repeat timing under stable load.`,
      ],
    };
  }
  return { status: findings.length ? "regression" : "passed", findings };
}
