import * as Cause from "effect/Cause";
import * as Effect from "effect/Effect";
import * as Exit from "effect/Exit";
import * as Option from "effect/Option";

/** Preserve typed failures and interruption while returning unexpected defects to the consuming workflow. */
export async function runRequestEffect<A, E extends Error>(
  operation: Effect.Effect<A, E>,
  options?: { readonly signal?: AbortSignal },
): Promise<A> {
  const exit = await Effect.runPromiseExit(operation, options);
  if (Exit.isSuccess(exit)) {
    return exit.value;
  }
  const failure = Cause.failureOption(exit.cause);
  if (Option.isSome(failure)) {
    throw failure.value;
  }
  if (Cause.isInterruptedOnly(exit.cause)) {
    throw new DOMException("The request was cancelled.", "AbortError");
  }
  const defect = Cause.squash(exit.cause);
  throw defect;
}
