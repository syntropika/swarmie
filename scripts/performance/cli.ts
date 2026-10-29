import { parseArgs } from "node:util";

import { compare, reportValue } from "./model";
import { run, selectCases } from "./runner";

const help = `Usage:
  bun run perf list [--suite quick|full] [--case <prefix>]
  bun run perf run --output <new-directory-outside-repository> [--suite quick|full] [--case <prefix>] [--samples 5] [--warmups 1] [--timeout-ms 180000] [--baseline <report.json>]
  bun run perf compare --baseline <report.json> --current <report.json>

Runs use synthetic repositories only. The native worker is built in release mode.
Each sample uses a fresh process; warmups warm filesystem/tool caches, not a retained JIT.
Relative gates: median +20%/2 ms, p95 +30%/2 ms, worker peak RSS +20%/4 MiB.
Native command/row/HTML counts must not increase. Timing CV above 20% requires a repeat.
Machine, workload, toolchain and sample policies must match. Baselines are never overwritten.
Measured RAM capacity may differ by at most 5%; the raw capacities remain in each report.
SSR measures server rendering; browser layout, scrolling and native UI remain separate.
SSR time is the mean of five warm renders per worker; cold render time is also retained.
Worker resources include fixture setup and cleanup. They are not a simultaneous tree peak.`;

function count(
  value: string | undefined,
  fallback: number,
  minimum: number,
  maximum: number,
): number {
  const result = value === undefined ? fallback : Number(value);
  if (!Number.isSafeInteger(result) || result < minimum || result > maximum) {
    throw new Error(`Expected an integer from ${minimum} to ${maximum}.`);
  }
  return result;
}

async function main() {
  const { values, positionals } = parseArgs({
    args: process.argv.slice(2),
    allowPositionals: true,
    options: {
      help: { type: "boolean" },
      suite: { type: "string" },
      case: { type: "string" },
      output: { type: "string" },
      baseline: { type: "string" },
      current: { type: "string" },
      samples: { type: "string" },
      warmups: { type: "string" },
      "timeout-ms": { type: "string" },
    },
  });
  if (values.help === true || positionals.length === 0) {
    console.log(help);
    return;
  }
  if (positionals.length !== 1) {
    throw new Error(help);
  }
  const suite = values.suite ?? "quick";
  if (suite !== "quick" && suite !== "full") {
    throw new Error("Unknown performance suite.");
  }
  const [operation] = positionals;
  if (operation === "list") {
    console.log(JSON.stringify(selectCases(suite, values.case ?? null), null, 2));
    return;
  }
  if (operation === "compare" && values.baseline !== undefined && values.current !== undefined) {
    const result = compare(
      reportValue(await Bun.file(values.baseline).json()),
      reportValue(await Bun.file(values.current).json()),
    );
    console.log(JSON.stringify(result, null, 2));
    process.exitCode = result.status === "passed" ? 0 : 1;
    return;
  }
  if (operation !== "run" || values.output === undefined) {
    throw new Error(help);
  }
  const passed = await run({
    suite,
    output: values.output,
    samples: count(values.samples, 5, 5, 30),
    warmups: count(values.warmups, 1, 1, 10),
    timeoutMs: count(values["timeout-ms"], 180_000, 100, 600_000),
    baseline: values.baseline ?? null,
    filter: values.case ?? null,
  });
  process.exitCode = passed ? 0 : 1;
}

try {
  await main();
} catch (error: unknown) {
  console.error(error instanceof Error ? error.message : String(error));
  process.exitCode = 1;
}
