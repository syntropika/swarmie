import { spawn } from "node:child_process";
import { randomUUID } from "node:crypto";

import { diagnosticRedactor, diagnosticWriter } from "./diagnostics.mjs";
import { InputQueue } from "./input-queue.mjs";
import { TaskTracker } from "./tasks.mjs";
import { sdkOutputLines } from "./transport.mjs";

/** Owns one official SDK query; all provider transport remains inside the SDK. */
export class AgentSession {
  constructor(query, emit, redact = diagnosticRedactor()) {
    this.queryFactory = query;
    this.emit = emit;
    this.permissions = new Map();
    this.redact = redact;
    this.taskTracker = new TaskTracker(emit);
    this.taskStops = new Set();
    this.diagnostics = diagnosticWriter((text) => process.stderr.write(text), redact);
  }
  async initialize(params) {
    if (this.query) {
      throw new Error("Claude SDK session is already initialized");
    }
    this.input = new InputQueue();
    this.abort = new AbortController();
    this.taskTracker.sessionId = params.options?.sessionId ?? params.options?.resume ?? null;
    const options = {
      ...params.options,
      pathToClaudeCodeExecutable: params.cliExecutable,
      spawnClaudeCodeProcess: (launch) => this.spawnProcess(params, launch),
      abortController: this.abort,
      canUseTool: (name, input, context) => this.permission(name, input, context),
      stderr: (text) => this.diagnostics.write(text),
      hooks: Object.fromEntries(
        ["PreToolUse", "PostToolUse", "PostToolUseFailure"].map((name) => [
          name,
          [
            {
              hooks: [
                async (input) => {
                  await this.emit({ type: "sdk_hook", hook: name, data: input });
                  return {};
                },
              ],
            },
          ],
        ]),
      ),
    };
    this.query = this.queryFactory({ prompt: this.input, options });
    this.pump = (async () => {
      try {
        for await (const message of this.query) {
          await this.observeTasks(message);
          if (message.type === "result" && message.is_error) {
            const safe = { ...message };
            if (typeof safe.result === "string") {
              safe.result = this.redact(safe.result);
            }
            if (Array.isArray(safe.errors)) {
              safe.errors = safe.errors.map((error) => this.redact(error));
            }
            await this.emit(safe);
          } else {
            await this.emit(message);
          }
        }
        if (!this.closed) {
          await this.emit({ _runtime_error: "Claude SDK message stream ended unexpectedly" });
        }
      } catch (error) {
        if (!this.closed) {
          await this.emit({ _runtime_error: this.redact(error.message ?? error) });
        }
      } finally {
        this.close();
      }
    })();
    try {
      return await this.query.initializationResult();
    } catch (error) {
      this.close();
      throw error;
    }
  }
  spawnProcess(params, launch) {
    const child = spawn(
      params.cliArgs?.length ? params.cliExecutable : launch.command,
      [...(params.cliArgs ?? []), ...launch.args],
      {
        cwd: launch.cwd,
        env: launch.env,
        stdio: ["pipe", "pipe", "pipe"],
        signal: launch.signal,
      },
    );
    child.stdout = child.stdout.pipe(sdkOutputLines());
    child.stderr.on("data", (text) => this.diagnostics.write(text));
    child.stderr.on("end", () => this.diagnostics.flush());
    return child;
  }
  observeTasks(message) {
    return this.taskTracker.observe(message);
  }
  permission(name, input, context) {
    const id = randomUUID();
    const pending = Promise.withResolvers();
    const { resolve } = pending;
    {
      const cancel = () => {
        if (!this.permissions.delete(id)) {
          return;
        }
        this.emit({ type: "sdk_permission_cancel", request_id: id });
        resolve({ behavior: "deny", message: "Permission request cancelled" });
      };
      const finish = (value) => {
        context.signal.removeEventListener("abort", cancel);
        resolve(value);
      };
      this.permissions.set(id, { finish, cancel });
      context.signal.addEventListener("abort", cancel, { once: true });
      if (context.signal.aborted) {
        cancel();
        return pending.promise;
      }
      this.emit({
        type: "sdk_permission",
        request_id: id,
        request: {
          tool_name: name,
          input,
          tool_use_id: context.toolUseID,
          decision_reason: context.decisionReason,
          permission_suggestions: context.suggestions,
          blocked_path: context.blockedPath,
        },
      });
    }
    return pending.promise;
  }
  disposePermissions() {
    for (const [id, pending] of this.permissions) {
      this.permissions.delete(id);
      pending.finish({ behavior: "deny", message: "SDK session closed" });
      this.emit({ type: "sdk_permission_cancel", request_id: id });
    }
  }
  async call(method, params) {
    if (method === "initialize") {
      return this.initialize(params);
    }
    if (method === "close") {
      await this.shutdown();
      return {};
    }
    if (!this.query || this.closed) {
      throw new Error("Claude SDK session is closed or not initialized");
    }
    switch (method) {
      case "prompt": {
        this.input.push(params);
        return {};
      }
      case "permission": {
        const pending = this.permissions.get(params.id);
        if (!pending) {
          throw new Error("Unknown or expired Claude SDK permission");
        }
        this.permissions.delete(params.id);
        pending.finish(params.response);
        return {};
      }
      case "interrupt": {
        return (await this.query.interrupt()) ?? {};
      }
      case "set_model": {
        await this.query.setModel(params.model ?? undefined);
        return {};
      }
      case "set_permission_mode": {
        await this.query.setPermissionMode(params.mode);
        return {};
      }
      case "apply_flag_settings": {
        await this.query.applyFlagSettings(params.settings);
        return {};
      }
      case "get_usage": {
        return this.query.usage_EXPERIMENTAL_MAY_CHANGE_DO_NOT_RELY_ON_THIS_API_YET({
          skipBehaviors: true,
        });
      }
      case "rewind_files": {
        return this.query.rewindFiles(params.user_message_id, { dryRun: params.dry_run });
      }
      case "mcp_set_servers": {
        return this.query.setMcpServers(params.servers);
      }
      case "tasks": {
        return this.taskTracker.list();
      }
      case "stop_task": {
        return this.stopTask(params.taskId);
      }
      default: {
        throw new Error(`Unsupported Claude SDK operation: ${method}`);
      }
    }
  }
  async stopTask(taskId) {
    if (!this.taskTracker.running(taskId)) {
      throw new Error("Unknown or inactive Claude SDK task");
    }
    if (this.taskStops.has(taskId)) {
      throw new Error("Claude SDK task stop is already pending");
    }
    this.taskStops.add(taskId);
    try {
      await this.query.stopTask(taskId);
      return {};
    } finally {
      this.taskStops.delete(taskId);
    }
  }
  close() {
    if (this.closed) {
      return;
    }
    this.closed = true;
    this.disposePermissions();
    this.input?.close();
    this.query?.close();
    this.abort?.abort();
    this.diagnostics.flush();
  }
  async shutdown() {
    this.close();
    if (this.query?.[Symbol.asyncDispose]) {
      await this.query[Symbol.asyncDispose]();
    }
  }
}
