import { useState } from "react";
import type { ReactNode } from "react";

import type { PlanStep } from "../agents";
import type { ModelOption } from "../composer";
import type { FileDiffRow } from "../file-diff";
import type { SourceReference } from "../knowledge";
import type { ThreadListItem } from "../thread";

export function ExampleFrame({ children }: { children: ReactNode }) {
  return (
    <div className="mx-auto max-w-3xl space-y-5 p-2">
      <p className="text-xs text-text-muted">Interactive design system example · Synthetic data</p>
      {children}
    </div>
  );
}

export function ExampleReceipt({ receipt }: { receipt: string }) {
  return receipt.length > 0 ? (
    <output
      className="block rounded-control bg-success-soft p-3 text-ui text-success"
      aria-live="polite"
    >
      {receipt}
    </output>
  ) : null;
}

export function useExampleAction() {
  const [receipt, setReceipt] = useState("");
  return {
    receipt,
    record: (action: string) => {
      setReceipt(`Demo action: ${action}`);
    },
  };
}

export const sources = [
  {
    id: "design",
    title: "Swarmie design system",
    href: "https://github.com/syntropika/swarmie/blob/main/docs/client-runtime/DESIGN.md",
    domain: "github.com",
    excerpt: "Warm working surfaces and clear responsibility help people follow the work.",
  },
  {
    id: "architecture",
    title: "Swarmie architecture",
    href: "https://github.com/syntropika/swarmie/blob/main/docs/client-runtime/docs/architecture.md",
    domain: "github.com",
    excerpt: "Swarmie owns interfaces and contracts; providers connect through adapters.",
  },
] as const satisfies readonly SourceReference[];

export const models = [
  { id: "balanced", name: "Balanced", provider: "Demo provider", detail: "Everyday work" },
  { id: "deep", name: "Deep reasoning", provider: "Demo provider", detail: "Complex decisions" },
  { id: "fast", name: "Fast", provider: "Local demo", detail: "Short responses" },
] as const satisfies readonly ModelOption[];

export const diffRows: readonly FileDiffRow[] = [
  {
    id: "context",
    kind: "context",
    oldLine: 12,
    newLine: 12,
    text: "export function sessionTitle(session) {",
  },
  {
    id: "removed",
    kind: "removed",
    oldLine: 13,
    newLine: null,
    text: "  return session.title || 'Untitled';",
  },
  {
    id: "added",
    kind: "added",
    oldLine: null,
    newLine: 13,
    text: "  return session.title.trim() || 'New session';",
  },
  { id: "closing", kind: "context", oldLine: 14, newLine: 14, text: "}" },
];

export const plan: readonly PlanStep[] = [
  {
    id: "inspect",
    label: "Read the session contracts",
    detail: "Confirm the current behavior",
    state: "completed",
    statusLabel: "Done",
  },
  {
    id: "implement",
    label: "Build the shared components",
    detail: "Keep labels and actions controlled",
    state: "running",
    statusLabel: "In progress",
  },
  {
    id: "verify",
    label: "Verify interaction and accessibility",
    state: "idle",
    statusLabel: "Next",
  },
];

export const threads: readonly ThreadListItem[] = [
  {
    id: "design",
    title: "Assistant interface design",
    detail: "You and Swarmie · 4 minutes ago",
    group: "Pinned",
    pinned: true,
  },
  {
    id: "session",
    title: "Session architecture",
    detail: "You and Swarmie · Today",
    group: "Today",
  },
  {
    id: "review",
    title: "Review the invitation flow",
    detail: "You and Swarmie · Yesterday",
    group: "Yesterday",
  },
];

/** Original scalable fixture artwork represents a workspace document, not a generated photo. */
export const demoImage = `data:image/svg+xml,${encodeURIComponent('<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 640 400"><rect width="640" height="400" fill="#DFE7F1"/><rect x="100" y="60" width="440" height="280" rx="16" fill="#FCFAF7"/><path d="M140 120h240M140 160h340M140 200h260M140 240h320M140 280h180" stroke="#4C6486" stroke-width="12"/><circle cx="480" cy="120" r="24" fill="#CDE9DE"/></svg>')}`;

export const chartSeries = [
  { id: "sessions", label: "Sessions", values: [12, 18, 16, 25, 32, 38] },
  { id: "tasks", label: "Tasks", values: [4, 8, 7, 14, 17, 22] },
];
export const chartLabels = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

/** A short synthesized tone, encoded as a WAV fixture; no external media request is needed. */
export const demoAudio =
  "data:audio/wav;base64,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";
