/** DOM events start work synchronously; the owning capability receives every rejection. */
export function startAction(work: Promise<unknown>, onError: (error: unknown) => void): void {
  work.catch(onError);
}

export function errorMessage(
  value: unknown,
  fallback = "The action could not be completed.",
): string {
  if (value instanceof Error) {
    return value.message;
  }
  if (typeof value === "string") {
    return value;
  }
  return fallback;
}

/** Error ownership is fixed before an asynchronous DOM action starts. */
export function errorReporter(report: (message: string) => void): (error: unknown) => void {
  return (error) => {
    report(errorMessage(error));
  };
}
