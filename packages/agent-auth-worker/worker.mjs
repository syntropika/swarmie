import { createInterface } from "node:readline";
import { pathToFileURL } from "node:url";

const output = (frame) => process.stdout.write(`${JSON.stringify(frame)}\n`);
let controller;
let answer;
let active = false;
const loginTimeoutMs = 300_000;

export async function performLogin(request, notify, signal, prompt, modules = {}) {
  if (request.harness === "cursor") {
    const { Cursor } = modules.cursor ?? (await import("@cursor/sdk"));
    const result = await Cursor.auth.login({
      openBrowser: false,
      store: null,
      signal,
      apiKeyName: "Swarmie",
      onLoginUrl: (url) => notify({ type: "authorization", url }),
    });
    return { apiKey: result.apiKey, expiresAt: result.apiKeyExpiresAtMs };
  }
  if (request.harness === "pi") {
    let factory;
    if (request.provider === "openai-codex") {
      const module =
        modules.piOpenAI ?? (await import("@earendil-works/pi-ai/providers/openai-codex"));
      factory = module.openaiCodexProvider;
    } else if (request.provider === "github-copilot") {
      const module =
        modules.piGitHub ?? (await import("@earendil-works/pi-ai/providers/github-copilot"));
      factory = module.githubCopilotProvider;
    }
    if (!factory) {
      throw new Error("Unsupported native Pi OAuth provider");
    }
    const provider = factory();
    const credential = await provider.auth.oauth.login(
      {
        signal,
        prompt: async (input) => prompt(input),
        notify: (event) => {
          if (event.type === "auth_url") {
            notify({ type: "authorization", url: event.url });
          } else if (event.type === "device_code") {
            notify({
              type: "authorization",
              url: event.verificationUri,
              userCode: event.userCode,
              expiresInSeconds: event.expiresInSeconds,
            });
          } else if (event.type === "progress" || event.type === "info") {
            notify({ type: "progress", message: "Waiting for provider authentication" });
          }
        },
      },
      { agentName: "Swarmie" },
    );
    return { nativeCredential: credential };
  }
  throw new Error("Unsupported native SDK authentication tool");
}

async function processRequest(request) {
  if (request.operation === "check") {
    const { Cursor } = await import("@cursor/sdk");
    try {
      await Cursor.me({ apiKey: request.apiKey });
      output({ type: "validated", authState: "authenticated" });
    } catch {
      output({
        type: "validated",
        authState: "unknown",
        message: "Cursor could not confirm this API key",
      });
    }
    return;
  }
  if (request.operation === "cancel") {
    controller?.abort();
    answer?.reject(new Error("Cancelled"));
    return;
  }
  if (request.operation === "input") {
    if (!answer) {
      throw new Error("No pending authentication input");
    }
    const pending = answer;
    answer = undefined;
    pending.resolve(String(request.value ?? ""));
    return;
  }
  if (request.operation !== "login" || active) {
    throw new Error("Unsupported or concurrent authentication operation");
  }
  active = true;
  controller = new AbortController();
  const deadline = setTimeout(() => controller.abort(), loginTimeoutMs);
  try {
    const result = await performLogin(request, output, controller.signal, (prompt) => {
      output({
        type: "input",
        secret: prompt.type === "secret" || prompt.type === "manual_code",
        placeholder: prompt.placeholder,
        options: prompt.type === "select" ? prompt.options : undefined,
      });
      const deferred = Promise.withResolvers();
      const pending = {
        resolve: (value) => {
          cleanup();
          deferred.resolve(value);
        },
        reject: (error) => {
          cleanup();
          deferred.reject(error);
        },
      };
      function abort() {
        if (answer === pending) {
          answer = undefined;
        }
        pending.reject(new Error("Cancelled"));
      }
      function cleanup() {
        prompt.signal?.removeEventListener("abort", abort);
        controller.signal.removeEventListener("abort", abort);
      }

      answer = pending;
      prompt.signal?.addEventListener("abort", abort, { once: true });
      controller.signal.addEventListener("abort", abort, { once: true });
      return deferred.promise;
    });
    if (controller.signal.aborted) {
      output({ type: "cancelled" });
    } else {
      output({ type: "completed", ...result });
    }
  } catch {
    output({
      type: controller.signal.aborted ? "cancelled" : "failed",
      message: "Provider sign-in could not be completed",
    });
  } finally {
    clearTimeout(deadline);
    answer = undefined;
    active = false;
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const [major, minor] = process.versions.node.split(".").map(Number);
  if (major < 22 || (major === 22 && minor < 13)) {
    throw new Error("Agent authentication requires Node.js 22.13 or later");
  }
  const lines = createInterface({ input: process.stdin, terminal: false });
  lines.on("line", (line) => {
    if (line.length > 16_384) {
      controller?.abort();
      output({ type: "failed", message: "Authentication request exceeds limit" });
      return;
    }
    try {
      void processRequest(JSON.parse(line)).catch(() =>
        output({ type: "failed", message: "Invalid authentication operation" }),
      );
    } catch {
      output({ type: "failed", message: "Invalid authentication request" });
    }
  });
  lines.on("close", () => {
    controller?.abort();
    answer?.reject(new Error("Input closed"));
  });
}
