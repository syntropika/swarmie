import { GitRemote } from "@swarmie/plugin-git-browser";

import type { SubmissionAdmission } from "./submissions";

/** Validate the captured checkout before dispatch without changing its node. */
export const admitGitContext: SubmissionAdmission = async ({ context, connection, signal }) => {
  if (context?.owner !== "git") {
    return context;
  }
  signal.throwIfAborted();
  const response = await new GitRemote(connection).request({
    kind: "discover",
    path: context.path,
  });
  signal.throwIfAborted();
  if (
    response.kind !== "repository" ||
    response.repository === null ||
    response.repository.bare ||
    response.repository.root === null
  ) {
    throw new Error("The captured Git checkout is no longer available.");
  }
  const { repository } = response;
  const identity = JSON.stringify([repository.root, repository.git_dir, repository.common_dir]);
  if (typeof context.identity === "string" && context.identity !== identity) {
    throw new Error(
      "The conversation directory now points to another checkout. Start a new conversation to use it.",
    );
  }
  return {
    ...context,
    version: 1,
    principal: connection.principal,
    path: repository.root ?? context.path,
    identity,
  };
};
