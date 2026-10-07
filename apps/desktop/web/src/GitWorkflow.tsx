import { Disclosure } from "@/components/ui/disclosure";
import { useState } from "react";
import { Button } from "@/components/ui/button";
import { Select } from "@/components/ui/select";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import type { Thread } from "./protocol";
import type { WorkspaceStore } from "./workspace";
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
export function GitWorkflow({
  store,
  thread,
}: {
  store: WorkspaceStore;
  thread: Thread;
}) {
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
  const [viewed, setViewed] = useState<
    { path: string; viewerViewedState: string }[]
  >([]);
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
    } catch (e) {
      store.setError(e);
    } finally {
      setBusy(false);
    }
  };
  const generate = async (kind: "commit" | "pr") => {
    const prepared = await store.extension<{
      prompt: string;
      truncated: boolean;
    }>("text.prompt", { ...args, kind, base });
    const text = await store.rpc<string>("runtime/generate", {
      harness: thread.harness,
      cwd: thread.worktreeId,
      model: thread.model || null,
      config: thread.config ?? {},
      text: prepared.prompt,
    });
    if (kind === "commit") setMessage(text.trim());
    else {
      const parsed = JSON.parse(
        text.replace(/^```(?:json)?\s*/, "").replace(/\s*```$/, ""),
      );
      if (typeof parsed.title !== "string" || typeof parsed.body !== "string")
        throw new Error(
          "The harness did not return a PR title and description. Enter them below.",
        );
      setTitle(parsed.title);
      setBody(parsed.body);
    }
    setReceipt(
      prepared.truncated
        ? "Generated from a truncated diff. Review the text before publishing."
        : "Generated text is ready to review.",
    );
  };
  const load = async (selectedNumber?: number) => {
    const pr = await store.extension<PullRequest>(
      "pr.status",
      selectedNumber ? { ...args, number: selectedNumber } : lookupArgs,
    );
    const selector = { ...args, number: pr.number };
    const [discussion, status, files] = await Promise.all([
      store.extension<{ comments: Comment[]; reviews: Comment[] }>(
        "pr.comments",
        selector,
      ),
      store.extension<typeof checks>("pr.checks", selector),
      store.extension<{ files: typeof viewed }>("pr.viewed.list", selector),
    ]);
    setPullRequest(pr);
    setNumber(String(pr.number));
    setComments([...discussion.comments, ...discussion.reviews]);
    setChecks(status);
    setViewed(files.files);
    store.updateThread(thread.id, {
      linkedPr: { number: pr.number, url: pr.url },
    });
  };
  return (
    <div className="panel-body">
      <h3>Commit and push</h3>
      <label>
        Commit message
        <Textarea
          value={message}
          onChange={(e) => setMessage(e.target.value)}
        />
      </label>
      <div className="permission-actions">
        <Button
          variant="ghost"
          className="small-button"
          disabled={busy}
          onClick={() => void run(() => generate("commit"))}
        >
          Generate message
        </Button>
        <Button
          variant="ghost"
          className="small-button"
          disabled={busy || !message.trim()}
          onClick={() =>
            void run(async () => {
              const result = await store.extension<{ head: string }>("commit", {
                ...args,
                message,
              });
              setReceipt(`Committed staged changes: ${result.head}`);
              setMessage("");
            })
          }
        >
          Commit staged changes
        </Button>
        <Button
          variant="ghost"
          className="small-button"
          disabled={busy}
          onClick={() =>
            void run(async () => {
              const result = await store.extension<{
                remote: string;
                branch: string;
              }>("push", args);
              setReceipt(`Pushed ${result.branch} to ${result.remote}.`);
            })
          }
        >
          Push branch
        </Button>
      </div>
      {receipt && (
        <p role="status" className="panel-note">
          {receipt}
        </p>
      )}
      <hr />
      <h3>Create a draft pull request</h3>
      <label>
        Base branch
        <Input value={base} onChange={(e) => setBase(e.target.value)} />
      </label>
      <label>
        Title
        <Input value={title} onChange={(e) => setTitle(e.target.value)} />
      </label>
      <label>
        Description
        <Textarea value={body} onChange={(e) => setBody(e.target.value)} />
      </label>
      <div className="permission-actions">
        <Button
          variant="ghost"
          className="small-button"
          disabled={busy || !base.trim()}
          onClick={() => void run(() => generate("pr"))}
        >
          Generate title and description
        </Button>
        <Button
          variant="ghost"
          className="small-button"
          disabled={busy || !title.trim()}
          onClick={() =>
            void run(async () => {
              const result = await store.extension<{ url: string }>(
                "pr.create",
                { ...args, title, body, base },
              );
              const match = /\/pull\/(\d+)/.exec(result.url);
              if (match) setNumber(match[1]);
              store.updateThread(thread.id, {
                linkedPr: {
                  number: match ? Number(match[1]) : undefined,
                  url: result.url,
                },
              });
              setReceipt(`Draft PR created: ${result.url}`);
            })
          }
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
      <label>
        PR number
        <Input
          type="number"
          min="1"
          value={number}
          onChange={(e) => setNumber(e.target.value)}
          placeholder="Current branch PR"
        />
      </label>
      <Button
        variant="ghost"
        className="small-button"
        disabled={busy}
        onClick={() => void run(load)}
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
            {pullRequest.reviewDecision
              ? ` · ${pullRequest.reviewDecision}`
              : ""}
          </p>
          <div className="check-results">
            {checks.map((check, i) => (
              <div key={i}>
                {check.link ? (
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
            <Disclosure.Summary>Reviewed files</Disclosure.Summary>
            {viewed.map((file) => (
              <Checkbox key={file.path} label={file.path}
                  checked={file.viewerViewedState === "VIEWED"}
                  disabled={busy}
                  onChange={(checked) =>
                    void run(async () => {
                      await store.extension("pr.viewed", {
                        ...prArgs,
                        path: file.path,
                        viewed: checked,
                      });
                      await load(pullRequest?.number);
                    })
                  }
                />
            ))}
          </Disclosure>
          <Disclosure>
            <Disclosure.Summary>Comments and reviews · {comments.length}</Disclosure.Summary>
            {comments.map((item, i) => (
              <div className="pr-comment" key={i}>
                <strong>
                  {item.author?.login ?? "Reviewer"}
                  {item.state ? ` · ${item.state}` : ""}
                </strong>
                <p>{item.body}</p>
                {item.url && (
                  <a href={item.url} target="_blank" rel="noreferrer">
                    View discussion
                  </a>
                )}
              </div>
            ))}
          </Disclosure>
          <label>
            Comment or review
            <Textarea
              value={comment}
              onChange={(e) => setComment(e.target.value)}
            />
          </label>
          <div className="permission-actions">
            <Button
              variant="ghost"
              className="small-button"
              disabled={busy || !comment.trim()}
              onClick={() =>
                void run(async () => {
                  await store.extension("pr.comment", {
                    ...prArgs,
                    body: comment,
                  });
                  setComment("");
                  await load(pullRequest?.number);
                })
              }
            >
              Post comment
            </Button>
            <Select label="Review decision" value={reviewEvent} onChange={setReviewEvent}
              options={[{ id: "comment", label: "Comment review" }, { id: "approve", label: "Approve review" }, { id: "request_changes", label: "Request changes" }]} />
            <Button
              variant="ghost"
              className="small-button"
              disabled={busy || (reviewEvent !== "approve" && !comment.trim())}
              onClick={() =>
                void run(async () => {
                  await store.extension("pr.review", {
                    ...prArgs,
                    event: reviewEvent,
                    body: comment,
                  });
                  setComment("");
                  await load(pullRequest?.number);
                })
              }
            >
              Submit review
            </Button>
          </div>
        </>
      )}
    </div>
  );
}
