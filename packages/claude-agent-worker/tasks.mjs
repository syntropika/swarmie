const terminal = new Set(["completed", "failed", "stopped"]);
const statuses = new Set(["running", "completed", "failed", "stopped", "unknown"]);
const patchStatuses = {
  pending: "unknown",
  paused: "unknown",
  running: "running",
  completed: "completed",
  failed: "failed",
  killed: "stopped",
};
function hasControl(value) {
  return [...value].some(
    (character) => character.codePointAt(0) < 32 || character.codePointAt(0) === 127,
  );
}
function validTaskId(id) {
  return typeof id === "string" && id.length > 0 && id.length <= 512 && !hasControl(id);
}
function text(value) {
  return typeof value === "string" ? value.slice(0, 2048) : undefined;
}
function taskStatus(message, patch, current) {
  let status = "running";
  if (message.subtype === "task_notification") {
    ({ status } = message);
  } else if (message.subtype === "task_updated") {
    status = patchStatuses[patch.status] ?? current?.task.status ?? "unknown";
  }
  return statuses.has(status) ? status : "unknown";
}
function taskEntry(message, current, run) {
  const patch = message.subtype === "task_updated" ? (message.patch ?? {}) : message;
  const status = taskStatus(message, patch, current);
  if (current && terminal.has(current.task.status) && !terminal.has(status)) {
    return undefined;
  }
  // Bound the complete list below the runtime's 8 MiB IPC record limit.
  const kind = text(message.task_type)?.slice(0, 256) ?? current?.task.kind;
  const summary = text(message.summary) ?? current?.task.summary;
  return {
    task: {
      id: message.task_id,
      description: text(patch.description) ?? current?.task.description ?? message.task_id,
      status,
      ...(kind ? { kind } : {}),
      ...(summary ? { summary } : {}),
    },
    run,
    background:
      typeof patch.is_backgrounded === "boolean"
        ? patch.is_backgrounded
        : (current?.background ?? false),
  };
}
function validRun(run) {
  return run === undefined || (typeof run === "string" && run.length <= 2048 && !hasControl(run));
}
function observedRun(previous, runId) {
  const run = typeof runId === "string" ? runId : null;
  if (previous?.run && (!run || run < previous.run)) {
    return undefined;
  }
  const newer = run && previous && run !== previous.run;
  return { run, current: newer ? undefined : previous };
}
/** Tracks native query facts; a stop receipt never invents a task outcome. */
export class TaskTracker {
  cache = new Map();
  sessionId = null;
  constructor(emit) {
    this.emit = emit;
  }
  list() {
    return structuredClone([...this.cache.values()].map((entry) => entry.task));
  }
  running(id) {
    return validTaskId(id) && this.cache.get(id)?.task.status === "running";
  }
  update(message) {
    if (!validTaskId(message?.task_id)) {
      return undefined;
    }
    if (!validRun(message.run_id)) {
      return undefined;
    }
    const observation = observedRun(this.cache.get(message.task_id), message.run_id);
    if (!observation) {
      return undefined;
    }
    const { run, current } = observation;
    const entry = taskEntry(message, current, run);
    if (!entry) {
      return undefined;
    }
    this.cache.set(entry.task.id, entry);
    while (this.cache.size > 256) {
      const oldest =
        [...this.cache].find(
          ([, item]) => terminal.has(item.task.status) || item.task.status === "unknown",
        )?.[0] ?? this.cache.keys().next().value;
      this.cache.delete(oldest);
    }
    if (!current || JSON.stringify(entry.task) !== JSON.stringify(current.task)) {
      return { type: "sdk_task", task: entry.task };
    }
    return undefined;
  }
  backgroundUpdates(tasks) {
    const live = new Set();
    const updates = [];
    for (const task of tasks) {
      if (validTaskId(task?.task_id)) {
        live.add(task.task_id);
      }
      const update = this.update({ ...task, subtype: "task_started", is_backgrounded: true });
      if (update) {
        updates.push(update);
      }
    }
    for (const entry of this.cache.values()) {
      if (entry.background && entry.task.status === "running" && !live.has(entry.task.id)) {
        entry.task = { ...entry.task, status: "unknown" };
        updates.push({ type: "sdk_task", task: entry.task });
      }
    }
    return updates;
  }
  async observe(message) {
    if (message.type !== "system") {
      return;
    }
    if (message.subtype === "init" && typeof message.session_id === "string") {
      if (this.sessionId !== message.session_id) {
        this.cache.clear();
      }
      this.sessionId = message.session_id;
      return;
    }
    if (!this.sessionId || message.session_id !== this.sessionId) {
      return;
    }
    let updates = [];
    if (message.subtype === "background_tasks_changed" && Array.isArray(message.tasks)) {
      updates = this.backgroundUpdates(message.tasks);
    } else if (
      ["task_started", "task_progress", "task_updated", "task_notification"].includes(
        message.subtype,
      )
    ) {
      const update = this.update(message);
      if (update) {
        updates.push(update);
      }
    }
    for await (const update of updates) {
      await this.emit(update);
    }
  }
}
