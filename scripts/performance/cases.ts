export const fixtureVersion = 1;

export type PerformanceCase = {
  id: string;
  engine: "rust" | "bun";
  operation: string;
  size: number;
  scope: string;
};

const nativeCases: PerformanceCase[] = [
  {
    id: "git.inventory",
    engine: "rust",
    operation: "inventory",
    size: 1000,
    scope: "discovery and complete status/files pagination",
  },
  {
    id: "git.archive",
    engine: "rust",
    operation: "archive",
    size: 25,
    scope: "capture and verified restore, including staged/working/ignored content",
  },
  {
    id: "git.history",
    engine: "rust",
    operation: "history",
    size: 30,
    scope: "native revision selection and gix metadata decoding",
  },
];

const renderCases: PerformanceCase[] = [
  {
    id: "ui.diff.unified",
    engine: "bun",
    operation: "unified",
    size: 5000,
    scope:
      "mean of five warm production SSR renders; cold render also retained; excludes browser layout/highlighting",
  },
  {
    id: "ui.diff.split",
    engine: "bun",
    operation: "split",
    size: 5000,
    scope:
      "mean of five warm production SSR renders; cold render also retained; excludes browser layout/highlighting",
  },
  {
    id: "ui.code.preview",
    engine: "bun",
    operation: "preview",
    size: 5000,
    scope:
      "mean of five warm production SSR renders; cold render also retained; excludes browser layout/highlighting",
  },
  {
    id: "ui.diff.syntax",
    engine: "bun",
    operation: "syntax",
    size: 200,
    scope: "cold grammar loading and old/new hunk tokenization",
  },
];

export function cases(suite: "quick" | "full"): PerformanceCase[] {
  const larger = new Map([
    ["inventory", 5000],
    ["archive", 250],
    ["history", 200],
  ]);
  return [
    ...nativeCases.map((entry) => ({
      ...entry,
      size: suite === "full" ? (larger.get(entry.operation) ?? entry.size) : entry.size,
    })),
    ...renderCases,
  ];
}
