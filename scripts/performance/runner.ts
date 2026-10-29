import { mkdir } from "node:fs/promises";
import path from "node:path";

import { cases, fixtureVersion } from "./cases";
import type { PerformanceCase } from "./cases";
import { compare, finite, reportValue, statistics } from "./model";
import type { CaseResult, PerformanceReport } from "./model";
import { externalPath, prepare, sample, sourceIdentity } from "./workers";
import type { Sample } from "./workers";

type BudgetPolicy = Record<string, Record<string, number>>;
type RunOptions = {
  suite: "quick" | "full";
  output: string;
  samples: number;
  warmups: number;
  timeoutMs: number;
  baseline: string | null;
  filter: string | null;
};

export function selectCases(suite: "quick" | "full", filter: string | null) {
  const selected = cases(suite).filter((entry) => filter === null || entry.id.startsWith(filter));
  if (selected.length === 0) {
    throw new Error("No performance case matches the selected prefix.");
  }
  return selected;
}

function summarize(entry: PerformanceCase, samples: Sample[]): CaseResult {
  function values(select: (value: Sample) => Record<string, number>) {
    const [first] = samples;
    if (first === undefined) {
      throw new Error("Missing measurement samples.");
    }
    const keys = Object.keys(select(first)).toSorted();
    if (
      samples.some(
        (value) => JSON.stringify(Object.keys(select(value)).toSorted()) !== JSON.stringify(keys),
      )
    ) {
      throw new Error("Metric availability changed between samples.");
    }
    return Object.fromEntries(
      keys.map((key) => [key, statistics(samples.map((value) => finite(select(value)[key])))]),
    );
  }
  return {
    ...entry,
    metrics: values((value) => value.metrics),
    resources: values((value) => value.resources),
  };
}

// Stable work budgets prevent silent increases without comparing unlike machines.
export function budgetFindings(report: PerformanceReport): string[] {
  const limits = {
    "git.inventory": { native_commands: report.suite === "full" ? 719 : 167 },
    "git.archive": { native_commands: report.suite === "full" ? 1914 : 339 },
    "git.history": { native_commands: 12 },
    "ui.diff.unified": { rendered_rows: 5002, html_bytes: 1_604_516 },
    "ui.diff.split": { rendered_rows: 5002, html_bytes: 2_590_089 },
    "ui.code.preview": { rendered_rows: 5001, html_bytes: 869_186 },
  } satisfies BudgetPolicy;
  return report.cases.flatMap((entry) =>
    Object.entries(Object.entries(limits).find(([id]) => id === entry.id)?.[1] ?? {}).flatMap(
      ([key, limit]) => {
        const metric = entry.metrics[key];
        if (metric === undefined) {
          return [`${entry.id}: missing budget metric ${key}.`];
        }
        return metric.max > limit ? [`${entry.id}: ${key}=${metric.max} exceeds ${limit}.`] : [];
      },
    ),
  );
}

async function collect(options: RunOptions, binary: string, output: string) {
  const selected = selectCases(options.suite, options.filter);
  const measured = new Map<string, Sample[]>();
  for (const entry of selected) {
    measured.set(entry.id, []);
  }
  const jobs = Array.from({ length: options.warmups + options.samples }, (_, round) => {
    const offset = round % selected.length;
    return [...selected.slice(offset), ...selected.slice(0, offset)].map((entry) => ({
      entry,
      round,
      phase: round < options.warmups ? "warmup" : "sample",
    }));
  }).flat();
  let interrupted = false;
  const stop = () => {
    interrupted = true;
  };
  // Samples must remain sequential. Overlapping them would measure contention.
  async function measureAt(index: number): Promise<void> {
    if (interrupted) {
      throw new Error(
        "Performance run interrupted; the current worker settled and no further sample was started.",
      );
    }
    const job = jobs[index];
    if (job === undefined) {
      return;
    }
    const { entry, round, phase } = job;
    const result = await sample(entry, binary, path.join(output, "fixtures"), options.timeoutMs);
    await Bun.write(
      path.join(output, `${phase}-${round}-${entry.id}.json`),
      JSON.stringify(result, null, 2),
    );
    if (phase === "sample") {
      measured.get(entry.id)?.push(result);
    }
    console.log(`${phase} ${round + 1}: ${entry.id} ${result.metrics.elapsed_ms?.toFixed(2)} ms`);
    await measureAt(index + 1);
  }
  process.on("SIGINT", stop);
  process.on("SIGTERM", stop);
  try {
    await measureAt(0);
  } finally {
    process.off("SIGINT", stop);
    process.off("SIGTERM", stop);
  }
  return selected.map((entry) => summarize(entry, measured.get(entry.id) ?? []));
}

export async function run(options: RunOptions): Promise<boolean> {
  selectCases(options.suite, options.filter);
  const baseline =
    options.baseline === null ? null : reportValue(await Bun.file(options.baseline).json());
  const output = await externalPath(options.output);
  await mkdir(path.dirname(output), { recursive: true });
  await mkdir(output);
  await mkdir(path.join(output, "fixtures"));
  try {
    const prepared = await prepare();
    const measured = await collect(options, prepared.binary, output);
    const { nativeBinarySha256, ...initialSource } = prepared.source;
    if (JSON.stringify(initialSource) !== JSON.stringify(await sourceIdentity())) {
      throw new Error(
        "The source changed during measurement. Results were retained but cannot establish a baseline.",
      );
    }
    const report: PerformanceReport = {
      schema: 1,
      fixtureVersion,
      suite: options.suite,
      samples: options.samples,
      warmups: options.warmups,
      source: { ...initialSource, nativeBinarySha256 },
      environment: prepared.environment,
      cases: measured,
    };
    await Bun.write(path.join(output, "report.json"), JSON.stringify(report, null, 2));
    const budgets = budgetFindings(report);
    await Bun.write(
      path.join(output, "budgets.json"),
      JSON.stringify(
        { status: budgets.length ? "regression" : "passed", findings: budgets },
        null,
        2,
      ),
    );
    let comparisonPassed = true;
    if (baseline !== null) {
      const result = compare(baseline, report);
      await Bun.write(
        path.join(output, "comparison.json"),
        JSON.stringify({ ...result, baseline: baseline.source, current: report.source }, null, 2),
      );
      comparisonPassed = result.status === "passed";
      console.log(`Baseline comparison: ${result.status}. ${result.findings.join(" ")}`);
    }
    for (const entry of report.cases) {
      console.log(
        `${entry.id}: median=${entry.metrics.elapsed_ms?.median.toFixed(2)} ms, p95=${entry.metrics.elapsed_ms?.p95.toFixed(2)} ms`,
      );
    }
    console.log(`Work budgets: ${budgets.length ? "regression" : "passed"}. ${budgets.join(" ")}`);
    console.log(
      `Report: ${path.join(output, "report.json")}. Worker CPU/RSS include setup and cleanup; they are not a simultaneous process-tree peak. SSR does not measure browser layout or scrolling.`,
    );
    return budgets.length === 0 && comparisonPassed;
  } catch (error: unknown) {
    await Bun.write(
      path.join(output, "failure.json"),
      JSON.stringify(
        { status: "failed", message: error instanceof Error ? error.message : String(error) },
        null,
        2,
      ),
    );
    throw error;
  }
}
