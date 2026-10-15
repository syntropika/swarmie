/** Consume callback-started work; workflows retain errors and navigation reports them through the host. */
export function observeBackgroundTask(task: Promise<unknown>): void {
  task.catch(() => {
    // The owner retains the visible failure.
  });
}
