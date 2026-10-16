import { Button, Popover, Tooltip, TooltipContent } from "@swarmie/ui-web";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ListTodo, LoaderCircle, RefreshCw, Square, X } from "lucide-react";
import { useEffect, useRef, useState, useSyncExternalStore } from "react";

import { startAction } from "~/src/action-boundary";
import { nodeKey } from "~/src/node-scope";
import type { AgentTask, Thread } from "~/src/protocol";
import type { WorkspaceStore } from "~/src/workspace";

const labels: Record<AgentTask["status"], string> = {
  running: "Running",
  completed: "Completed",
  failed: "Failed",
  stopped: "Stopped",
  unknown: "Status unavailable",
};
type TaskState = { tasks: AgentTask[]; requested: string[] };
type Props = { store: WorkspaceStore; thread: Thread; connected: boolean };
export function BackgroundTasks(props: Props) {
  const state = useSyncExternalStore(props.store.subscribe, props.store.getSnapshot);
  const scope = state.node ? nodeKey(state.node) : "";
  const available =
    props.thread.sessionId !== undefined &&
    props.thread.sessionId.length > 0 &&
    props.thread.capabilities?.backgroundTasks?.availability === "available" &&
    props.thread.state !== "closed";
  if (!available) {
    return null;
  }
  return (
    <TaskPopover
      key={`${scope}:${props.thread.id}:${props.thread.sessionId}:${props.connected}`}
      {...props}
      scope={scope}
    />
  );
}
function TaskPopover({ store, thread, connected, scope }: Props & { scope: string }) {
  const [open, setOpen] = useState(false);
  const [actionError, setActionError] = useState("");
  const mounted = useRef(false);
  const client = useQueryClient();
  const key = ["background-tasks", scope, thread.sessionId];
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);
  const current = () => {
    const latest = store.getSnapshot();
    return (
      latest.connected &&
      latest.node !== undefined &&
      nodeKey(latest.node) === scope &&
      latest.threads.some(
        (item) =>
          item.id === thread.id && item.sessionId === thread.sessionId && item.state !== "closed",
      )
    );
  };
  const query = useQuery({
    queryKey: key,
    enabled: open && connected && scope.length > 0,
    queryFn: async (): Promise<TaskState> => {
      const tasks = await store.tasks(thread.id);
      const previous = client.getQueryData<TaskState>(key);
      return {
        tasks,
        requested: (previous?.requested ?? []).filter((id) =>
          tasks.some((task) => task.id === id && task.status === "running"),
        ),
      };
    },
    refetchInterval: open && connected ? 2000 : false,
  });
  const stop = useMutation({
    mutationFn: async (taskId: string) => {
      if (!current()) {
        throw new Error("The task's session is no longer available");
      }
      await store.stopTask(thread.id, taskId);
    },
    onSuccess: (_receipt, taskId) => {
      if (!mounted.current || !current()) {
        return;
      }
      client.setQueryData<TaskState>(key, (previous) =>
        previous === undefined
          ? undefined
          : { ...previous, requested: [...new Set([...previous.requested, taskId])] },
      );
    },
  });
  const tasks = query.data?.tasks ?? [];
  const requested = query.data?.requested ?? [];
  let error = query.isError ? "Tasks could not be loaded. Try again." : "";
  if (actionError.length > 0) {
    error = actionError;
  }

  return (
    <Popover isOpen={open} onOpenChange={setOpen}>
      <Tooltip>
        <Button
          variant="tertiary"
          size="md"
          isIconOnly
          aria-label="Background tasks"
          disabled={!connected}
        >
          <ListTodo size={17} />
        </Button>

        <TooltipContent>Background tasks</TooltipContent>
      </Tooltip>
      <Popover.Content
        placement="bottom end"
        className="@container w-[26rem]"
        aria-label="Background tasks"
      >
        <Popover.Dialog aria-label="Background tasks">
          <div className="flex items-center justify-between gap-3">
            <h3 className="font-medium">Background tasks</h3>
            <Button
              variant="tertiary"
              size="sm"
              isIconOnly
              aria-label="Refresh background tasks"
              disabled={query.isFetching || stop.isPending || !connected}
              onClick={() => {
                setActionError("");
                startAction(query.refetch(), () => {
                  setActionError("Tasks could not be loaded. Try again.");
                });
              }}
            >
              <RefreshCw size={14} />
            </Button>
          </div>
          <Button
            variant="tertiary"
            isIconOnly
            aria-label="Close background tasks"
            onPress={() => {
              setOpen(false);
            }}
          >
            <X size={16} />
          </Button>
          <p className="mt-2 text-sm text-muted">
            Tasks started by this agent in the current session.
          </p>
          {query.isFetching && tasks.length === 0 && (
            <output className="mt-4 flex items-center gap-2 text-sm text-muted">
              <LoaderCircle size={14} className="animate-spin motion-reduce:animate-none" />
              Loading tasks…
            </output>
          )}
          {!query.isFetching && error.length === 0 && tasks.length === 0 && (
            <output className="mt-4 block text-sm text-muted">
              No background tasks have been observed.
            </output>
          )}
          <ul className="mt-3 max-h-80 overflow-y-auto" aria-label="Session background tasks">
            {tasks.map((task) => (
              <li
                key={task.id}
                className="flex items-start gap-3 border-b border-border py-3 last:border-b-0"
              >
                <div className="min-w-0 flex-1">
                  <p className="text-sm break-words">
                    {task.description.length > 0 ? task.description : "Background task"}
                  </p>
                  <p className="mt-1 text-xs text-muted">
                    {requested.includes(task.id) && task.status === "running"
                      ? "Stop requested · waiting for confirmation"
                      : labels[task.status]}
                  </p>
                  {task.summary !== undefined && (task.summary ?? "").length > 0 && (
                    <p className="mt-2 text-sm break-words text-muted">{task.summary}</p>
                  )}
                </div>
                {task.status === "running" && (
                  <Button
                    variant="outline"
                    size="sm"
                    disabled={stop.isPending || requested.includes(task.id) || !connected}
                    aria-label={`Stop ${task.description.length > 0 ? task.description : "background task"}`}
                    onClick={() => {
                      setActionError("");
                      startAction(stop.mutateAsync(task.id), () => {
                        setActionError(
                          "The task could not be stopped. Refresh its status and try again.",
                        );
                      });
                    }}
                  >
                    {stop.isPending && stop.variables === task.id ? (
                      <LoaderCircle size={13} className="animate-spin motion-reduce:animate-none" />
                    ) : (
                      <Square size={12} />
                    )}
                    Stop
                  </Button>
                )}
              </li>
            ))}
          </ul>
          {error.length > 0 && (
            <p role="alert" className="inline-error mt-3 text-sm">
              {error}
            </p>
          )}
        </Popover.Dialog>
      </Popover.Content>
    </Popover>
  );
}
