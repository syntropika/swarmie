import type { LocalStoreIdentity } from "@swarmie/contracts";

import { nodeKey } from "./node-scope";
import type { Detection } from "./protocol";

type SetupPreference = {
  completed: boolean;
  preferredHarness?: string;
  disabledHarnesses: string[];
};

// Setup preferences contain execution choices, never provider credentials.
export function readHarnessSetup(node?: LocalStoreIdentity): SetupPreference {
  if (!node) {
    return { completed: false, disabledHarnesses: [] };
  }
  try {
    const value: unknown = JSON.parse(
      localStorage.getItem(`swarmie.harness-setup.v1:${nodeKey(node)}`) ?? "null",
    );
    const saved = typeof value === "object" && value !== null ? value : {};
    return {
      completed: "completed" in saved && saved.completed === true,
      preferredHarness:
        "preferredHarness" in saved && typeof saved.preferredHarness === "string"
          ? saved.preferredHarness
          : undefined,
      disabledHarnesses:
        "disabledHarnesses" in saved && Array.isArray(saved.disabledHarnesses)
          ? [
              ...new Set<string>(
                saved.disabledHarnesses.filter((id: unknown) => typeof id === "string"),
              ),
            ]
          : [],
    };
  } catch {
    return { completed: false, disabledHarnesses: [] };
  }
}

export function completeHarnessSetup(node: LocalStoreIdentity, harness?: string) {
  localStorage.setItem(
    `swarmie.harness-setup.v1:${nodeKey(node)}`,
    JSON.stringify({
      ...readHarnessSetup(node),
      completed: true,
      preferredHarness: harness ?? readHarnessSetup(node).preferredHarness,
    }),
  );
}

export function setHarnessEnabled(node: LocalStoreIdentity, harness: string, enabled: boolean) {
  const saved = readHarnessSetup(node);
  const disabledHarnesses = enabled
    ? saved.disabledHarnesses.filter((id) => id !== harness)
    : [...new Set([...saved.disabledHarnesses, harness])];
  localStorage.setItem(
    `swarmie.harness-setup.v1:${nodeKey(node)}`,
    JSON.stringify({ ...saved, disabledHarnesses }),
  );
  return disabledHarnesses;
}

type HarnessGuide = { name: string; documentation?: string; command?: string; instruction: string };
type HarnessGuides = Record<string, HarnessGuide | undefined>;
const guides = {
  codex: {
    name: "Codex",
    documentation: "https://developers.openai.com/codex/cli",
    command: "codex login",
    instruction: "Run this command in a terminal and complete the browser sign-in.",
  },
  claude: {
    name: "Claude Code",
    documentation: "https://code.claude.com/docs/en/authentication",
    command: "claude",
    instruction:
      "Install Claude Code and Swarmie's Agent SDK support on this computer. Open Claude Code in a terminal and follow its sign-in flow, or use /login if it is already open.",
  },
  cursor: {
    name: "Cursor",
    documentation: "https://cursor.com/docs/sdk/bridge",
    instruction:
      "Install Cursor SDK Bridge on this computer and configure CURSOR_API_KEY with your Cursor API key. Then check again.",
  },
  grok: {
    name: "Grok Build",
    documentation: "https://docs.x.ai/build/overview",
    command: "grok login --device-auth",
    instruction: "Open Grok Build on this computer and follow its provider sign-in instructions.",
  },
  antigravity: {
    name: "Google Antigravity",
    documentation: "https://antigravity.google/docs/home",
    command: "agy",
    instruction:
      "Install Google Antigravity on this computer, then follow its account setup instructions.",
  },
  pi: {
    name: "Pi",
    documentation: "https://github.com/earendil-works/pi/tree/main/packages/coding-agent",
    command: "pi",
    instruction: "Open Pi in a terminal and use /login or configure your provider's API key.",
  },
  opencode: {
    name: "OpenCode",
    documentation: "https://docs.opencode.ai/docs/",
    command: "opencode",
    instruction: "Open OpenCode in a terminal and use /connect to configure your provider.",
  },
} satisfies HarnessGuides;

export const primaryAgentTools = [
  "codex",
  "claude",
  "cursor",
  "grok",
  "opencode",
  "antigravity",
  "pi",
];

export function harnessGuide(harness: string): HarnessGuide {
  return (
    Object.entries(guides).find(([id]) => id === harness)?.[1] ?? {
      name: harness,
      instruction: "Open this agent tool in a terminal and follow its provider setup instructions.",
    }
  );
}

export function harnessReadiness(harness: Detection) {
  if (!harness.available) {
    return {
      label: "Not installed",
      detail:
        (harness.message === undefined || harness.message === null || harness.message.length === 0
          ? undefined
          : harness.message) ?? "Install this agent on this computer to get started.",
      ready: false,
    };
  }
  if (harness.authState === "authenticated") {
    return { label: "Ready", detail: "Installed · Existing login detected", ready: true };
  }
  if (harness.authState === "unauthenticated") {
    return {
      label: "Sign in needed",
      detail:
        (harness.message === undefined || harness.message === null || harness.message.length === 0
          ? undefined
          : harness.message) ?? "Installed · Sign in to use your account",
      ready: false,
    };
  }
  return {
    label: "Installed",
    detail:
      (harness.message === undefined || harness.message === null || harness.message.length === 0
        ? undefined
        : harness.message) ?? "Authentication has not been confirmed.",
    ready: false,
  };
}

export function defaultHarness(harnesses: Detection[], node?: LocalStoreIdentity) {
  const { preferredHarness, disabledHarnesses } = readHarnessSetup(node);
  const enabled = harnesses.filter(
    (item) => item.available && !disabledHarnesses.includes(item.harness),
  );
  return (
    enabled.find((item) => item.harness === preferredHarness)?.harness ??
    enabled.find((item) => item.harness === "codex")?.harness ??
    enabled[0]?.harness
  );
}
