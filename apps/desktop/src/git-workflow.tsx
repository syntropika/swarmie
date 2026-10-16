import { Button, Checkbox, Disclosure, Input, TextArea, Select, ListBox } from "@swarmie/ui-web";
import { isValidElement, useState, useId } from "react";

import { startAction } from "~/src/action-boundary";
import { extensionResult } from "~/src/components/extension-result";
import type { Thread } from "~/src/protocol";
import { hasText, hasNumber } from "~/src/value-presence";
import type { WorkspaceStore } from "~/src/workspace";

type PullRequest = {
  number: number;
  url: string;
  title: string;
  body: string;
  state: string;
  isDraft: boolean;
  reviewDecision?: string;
  files?: { path: string }[];
};
type Comment = {
  body: string;
  author?: { login: string };
  url?: string;
  state?: string;
};
export function GitWorkflow({ store, thread }: { store: WorkspaceStore; thread: Thread }) {
  const fieldPrefix = useId();

  const [message, setMessage] = useState("");
  const [title, setTitle] = useState("");
  const [body, setBody] = useState("");
  const [base, setBase] = useState("main");
  const [number, setNumber] = useState("");
  const [pullRequest, setPullRequest] = useState<PullRequest>();
  const [comments, setComments] = useState<Comment[]>([]);
  const [checks, setChecks] = useState<
    { name: string; state?: string; bucket?: string; link?: string }[]
  >([]);
  const [viewed, setViewed] = useState<{ path: string; viewerViewedState: string }[]>([]);
  const [comment, setComment] = useState("");
  const [reviewEvent, setReviewEvent] = useState("comment");
  const [busy, setBusy] = useState(false);
  const [receipt, setReceipt] = useState("");
  const args = { projectId: thread.projectId, worktreeId: thread.worktreeId };
  const lookupArgs = { ...args, number: number ? Number(number) : undefined };
  const prArgs = { ...args, number: pullRequest?.number };
  const run = async (action: () => Promise<unknown>) => {
    setBusy(true);
    try {
      await action();
    } catch (error) {
      store.setError(error);
    } finally {
      setBusy(false);
    }
  };
  const generate = async (kind: "commit" | "pr") => {
    const result = await store.rpc("client/gitText", {
      kind,
      arguments: { ...args, base },
      launch: await store.sessionIntent(thread.id),
    });
    if (kind === "commit") {
      setMessage(result.draft.message ?? "");
    } else {
      setTitle(result.draft.title ?? "");
      setBody(result.draft.body ?? "");
    }
    setReceipt(
      result.truncated
        ? "Generated from a truncated diff. Review the text before publishing."
        : "Generated text is ready to review.",
    );
  };
  const load = async (selectedNumber?: number) => {
    const raw = await store.rpc(
      "client/prProjection",
      hasNumber(selectedNumber) ? { ...args, number: selectedNumber } : lookupArgs,
    );
    const {
      pr,
      discussion,
      checks: status,
      files,
    } = extensionResult<{
      pr: PullRequest;
      discussion: { comments: Comment[]; reviews: Comment[] };
      checks: typeof checks;
      files: typeof viewed;
    }>({
      type: "object",
      properties: {
        pr: {
          type: "object",
          properties: {
            number: { type: "number" },
            url: { type: "string" },
            title: { type: "string" },
            body: { type: "string" },
            state: { type: "string" },
            isDraft: { type: "boolean" },
            reviewDecision: { type: "string" },
            files: {
              type: "array",
              items: {
                type: "object",
                properties: { path: { type: "string" } },
                required: ["path"],
              },
            },
          },
          required: ["number", "url", "title", "body", "state", "isDraft"],
        },
        discussion: {
          type: "object",
          properties: {
            comments: {
              type: "array",
              items: {
                type: "object",
                properties: {
                  body: { type: "string" },
                  author: {
                    type: "object",
                    properties: { login: { type: "string" } },
                    required: ["login"],
                  },
                  url: { type: "string" },
                  state: { type: "string" },
                },
                required: ["body"],
              },
            },
            reviews: {
              type: "array",
              items: {
                type: "object",
                properties: {
                  body: { type: "string" },
                  author: {
                    type: "object",
                    properties: { login: { type: "string" } },
                    required: ["login"],
                  },
                  url: { type: "string" },
                  state: { type: "string" },
                },
                required: ["body"],
              },
            },
          },
          required: ["comments", "reviews"],
        },
        checks: {
          type: "array",
          items: {
            type: "object",
            properties: {
              name: { type: "string" },
              state: { type: "string" },
              bucket: { type: "string" },
              link: { type: "string" },
            },
            required: ["name"],
          },
        },
        files: {
          type: "array",
          items: {
            type: "object",
            properties: { path: { type: "string" }, viewerViewedState: { type: "string" } },
            required: ["path", "viewerViewedState"],
          },
        },
      },
      required: ["pr", "discussion", "checks", "files"],
    })(raw);
    setPullRequest(pr);
    setNumber(String(pr.number));
    setComments([...discussion.comments, ...discussion.reviews]);
    setChecks(status);
    setViewed(files);
    store.updateThread(thread.id, {
      linkedPr: { number: pr.number, url: pr.url },
    });
  };
  return (
    <div className="panel-body">
      <h3>Commit and push</h3>
      <label htmlFor={`${fieldPrefix}-field-1`}>
        Commit message
        <TextArea
          id={`${fieldPrefix}-field-1`}
          value={message}
          onChange={(e) => {
            setMessage(e.target.value);
          }}
        />
      </label>
      <div className="permission-actions">
        <Button
          variant="tertiary"
          size="sm"
          disabled={busy}
          onClick={() => {
            startAction(
              run(async () => {
                await generate("commit");
              }),
              store.setError.bind(store),
            );
          }}
        >
          Generate message
        </Button>
        <Button
          variant="tertiary"
          size="sm"
          disabled={busy || !message.trim()}
          onClick={() => {
            startAction(
              run(async () => {
                const result = await store.extension<{ head: string }>(
                  "commit",
                  {
                    ...args,
                    message,
                  },
                  extensionResult<{ head: string }>({
                    type: "object",
                    properties: { head: { type: "string" } },
                    required: ["head"],
                  }),
                );
                setReceipt(`Committed staged changes: ${result.head}`);
                setMessage("");
              }),
              store.setError.bind(store),
            );
          }}
        >
          Commit staged changes
        </Button>
        <Button
          variant="tertiary"
          size="sm"
          disabled={busy}
          onClick={() => {
            startAction(
              run(async () => {
                const result = await store.extension<{
                  remote: string;
                  branch: string;
                }>(
                  "push",
                  args,
                  extensionResult<{
                    remote: string;
                    branch: string;
                  }>({
                    type: "object",
                    properties: { remote: { type: "string" }, branch: { type: "string" } },
                    required: ["remote", "branch"],
                  }),
                );
                setReceipt(`Pushed ${result.branch} to ${result.remote}.`);
              }),
              store.setError.bind(store),
            );
          }}
        >
          Push branch
        </Button>
      </div>
      {receipt && <output className="panel-note">{receipt}</output>}
      <hr />
      <h3>Create a draft pull request</h3>
      <label htmlFor={`${fieldPrefix}-field-2`}>
        Base branch
        <Input
          id={`${fieldPrefix}-field-2`}
          value={base}
          onChange={(e) => {
            setBase(e.target.value);
          }}
        />
      </label>
      <label htmlFor={`${fieldPrefix}-field-3`}>
        Title
        <Input
          id={`${fieldPrefix}-field-3`}
          value={title}
          onChange={(e) => {
            setTitle(e.target.value);
          }}
        />
      </label>
      <label htmlFor={`${fieldPrefix}-field-4`}>
        Description
        <TextArea
          id={`${fieldPrefix}-field-4`}
          value={body}
          onChange={(e) => {
            setBody(e.target.value);
          }}
        />
      </label>
      <div className="permission-actions">
        <Button
          variant="tertiary"
          size="sm"
          disabled={busy || !base.trim()}
          onClick={() => {
            startAction(
              run(async () => {
                await generate("pr");
              }),
              store.setError.bind(store),
            );
          }}
        >
          Generate title and description
        </Button>
        <Button
          variant="tertiary"
          size="sm"
          disabled={busy || !title.trim()}
          onClick={() => {
            startAction(
              run(async () => {
                const result = await store.extension<{ url: string }>(
                  "pr.create",
                  {
                    ...args,
                    title,
                    body,
                    base,
                  },
                  extensionResult<{ url: string }>({
                    type: "object",
                    properties: { url: { type: "string" } },
                    required: ["url"],
                  }),
                );
                const match = /\/pull\/(?<number>\d+)/u.exec(result.url);
                if (match) {
                  setNumber(match.groups?.number ?? "");
                }
                store.updateThread(thread.id, {
                  linkedPr: {
                    number: match ? Number(match[1]) : undefined,
                    url: result.url,
                  },
                });
                setReceipt(`Draft PR created: ${result.url}`);
              }),
              store.setError.bind(store),
            );
          }}
        >
          Create draft PR
        </Button>
      </div>
      {thread.linkedPr && (
        <a href={thread.linkedPr.url} target="_blank" rel="noreferrer">
          Open linked pull request
        </a>
      )}
      <hr />
      <h3>Pull request review</h3>
      <label htmlFor={`${fieldPrefix}-field-5`}>
        PR number
        <Input
          id={`${fieldPrefix}-field-5`}
          type="number"
          min="1"
          value={number}
          onChange={(e) => {
            setNumber(e.target.value);
          }}
          placeholder="Current branch PR"
        />
      </label>
      <Button
        variant="tertiary"
        size="sm"
        disabled={busy}
        onClick={() => {
          startAction(run(load), store.setError.bind(store));
        }}
      >
        Load PR and checks
      </Button>
      {pullRequest && (
        <>
          <a href={pullRequest.url} target="_blank" rel="noreferrer">
            #{pullRequest.number} {pullRequest.title}
          </a>
          <p className="muted">
            {pullRequest.state}
            {pullRequest.isDraft ? " · draft" : ""}
            {hasText(pullRequest.reviewDecision) ? ` · ${pullRequest.reviewDecision}` : ""}
          </p>
          <div className="flex flex-col gap-2">
            {checks.map((check) => (
              <div key={`${check.name}:${check.link ?? ""}`}>
                {hasText(check.link) ? (
                  <a href={check.link} target="_blank" rel="noreferrer">
                    {check.name}
                  </a>
                ) : (
                  check.name
                )}
                <span>{check.state ?? check.bucket}</span>
              </div>
            ))}
          </div>
          <Disclosure>
            <Disclosure.Heading>
              <Button slot="trigger" variant="tertiary">
                <span>Reviewed files</span>
                <Disclosure.Indicator />
              </Button>
            </Disclosure.Heading>
            <Disclosure.Content>
              <Disclosure.Body>
                {viewed.map((file) => (
                  <Checkbox
                    key={file.path}
                    isSelected={file.viewerViewedState === "VIEWED"}
                    isDisabled={busy}
                    onChange={(checked) => {
                      startAction(
                        run(async () => {
                          await store.extension("pr.viewed", {
                            ...prArgs,
                            path: file.path,
                            viewed: checked,
                          });
                          await load(pullRequest?.number);
                        }),
                        store.setError.bind(store),
                      );
                    }}
                    aria-label={file.path}
                  >
                    <Checkbox.Content>
                      <Checkbox.Control>
                        <Checkbox.Indicator />
                      </Checkbox.Control>
                      {file.path}
                    </Checkbox.Content>
                  </Checkbox>
                ))}
              </Disclosure.Body>
            </Disclosure.Content>
          </Disclosure>
          <Disclosure>
            <Disclosure.Heading>
              <Button slot="trigger" variant="tertiary">
                <span>Comments and reviews · {comments.length}</span>
                <Disclosure.Indicator />
              </Button>
            </Disclosure.Heading>
            <Disclosure.Content>
              <Disclosure.Body>
                {comments.map((item) => (
                  <div
                    className="pr-comment"
                    key={item.url ?? `${item.author?.login ?? ""}:${item.state ?? ""}:${item.body}`}
                  >
                    <strong>
                      {item.author?.login ?? "Reviewer"}
                      {hasText(item.state) ? ` · ${item.state}` : ""}
                    </strong>
                    <p>{item.body}</p>
                    {hasText(item.url) && (
                      <a href={item.url} target="_blank" rel="noreferrer">
                        View discussion
                      </a>
                    )}
                  </div>
                ))}
              </Disclosure.Body>
            </Disclosure.Content>
          </Disclosure>
          <label htmlFor={`${fieldPrefix}-field-6`}>
            Comment or review
            <TextArea
              id={`${fieldPrefix}-field-6`}
              value={comment}
              onChange={(e) => {
                setComment(e.target.value);
              }}
            />
          </label>
          <div className="permission-actions">
            <Button
              variant="tertiary"
              size="sm"
              disabled={busy || !comment.trim()}
              onClick={() => {
                startAction(
                  run(async () => {
                    await store.extension("pr.comment", {
                      ...prArgs,
                      body: comment,
                    });
                    setComment("");
                    await load(pullRequest?.number);
                  }),
                  store.setError.bind(store),
                );
              }}
            >
              Post comment
            </Button>
            <Select
              aria-label="Review decision"
              value={reviewEvent}
              onChange={(key) => {
                if (typeof key === "string") {
                  setReviewEvent(key);
                }
              }}
              placeholder="Select…"
            >
              <Select.Trigger>
                <Select.Value />
                <Select.Indicator />
              </Select.Trigger>
              <Select.Popover>
                <ListBox aria-label="Review decision">
                  {[
                    { id: "comment", label: "Comment review" },
                    { id: "approve", label: "Approve review" },
                    { id: "request_changes", label: "Request changes" },
                  ].map((option) => (
                    <ListBox.Item
                      key={option.id}
                      id={option.id}
                      textValue={option.label}
                      isDisabled={"disabled" in option && option.disabled === true}
                    >
                      {"icon" in option && isValidElement(option.icon) ? option.icon : null}
                      {option.label}
                      <ListBox.ItemIndicator />
                    </ListBox.Item>
                  ))}
                </ListBox>
              </Select.Popover>
            </Select>
            <Button
              variant="tertiary"
              size="sm"
              disabled={busy || (reviewEvent !== "approve" && !comment.trim())}
              onClick={() => {
                startAction(
                  run(async () => {
                    await store.extension("pr.review", {
                      ...prArgs,
                      event: reviewEvent,
                      body: comment,
                    });
                    setComment("");
                    await load(pullRequest?.number);
                  }),
                  store.setError.bind(store),
                );
              }}
            >
              Submit review
            </Button>
          </div>
        </>
      )}
    </div>
  );
}
