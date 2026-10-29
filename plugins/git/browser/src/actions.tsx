import { Button, Input, Label, TextField, TextArea, VirtualList } from "@swarmie/ui-web";
import { useMemo, useState } from "react";

import type { Checkout } from "./checkout-data";
import type { GitOperation } from "./protocol";

type ActionProps = {
  checkout: Checkout;
  disabled: boolean;
  mutate: (operation: GitOperation) => Promise<boolean>;
  close: () => void;
  branchPrefix: string;
  commitGuidance: string;
};
function useSubmit({ mutate, close }: ActionProps) {
  const [failure, setFailure] = useState<string | null>(null);
  function submit(operation: GitOperation) {
    async function run() {
      try {
        if (await mutate(operation)) {
          close();
        }
      } catch (error: unknown) {
        setFailure(error instanceof Error ? error.message : "Unable to complete Git operation.");
      }
    }
    run().catch(console.error);
  }
  return { submit, error: failure };
}

export function HistoryPanel({ checkout }: { checkout: Checkout }) {
  if (checkout.commits.state === "unavailable") {
    return (
      <p role="alert" className="text-sm text-danger">
        History unavailable: {checkout.commits.error}
      </p>
    );
  }
  return (
    <>
      <h3 className="mb-3 text-sm font-medium">Recent history</h3>
      {checkout.commits.value.length === 0 ? (
        <p className="text-sm text-muted">No commits yet.</p>
      ) : (
        <ol className="space-y-3">
          {checkout.commits.value.map((commit) => (
            <li key={commit.oid} className="text-xs">
              <span className="font-mono text-muted">{commit.oid.slice(0, 8)}</span>{" "}
              <span>{commit.subject}</span>
              <p className="mt-1 text-muted">
                {commit.author} · {commit.authored_at}
              </p>
            </li>
          ))}
        </ol>
      )}
    </>
  );
}

export function BranchPanel(props: ActionProps) {
  const { submit, error } = useSubmit(props);
  const { checkout, disabled, close } = props;
  const [name, setName] = useState(props.branchPrefix);

  return (
    <form
      className="grid max-w-lg gap-3"
      onSubmit={(event) => {
        event.preventDefault();
        submit({ kind: "create_branch", name });
      }}
    >
      {error === null ? null : (
        <p role="alert" className="text-sm text-danger">
          {error}
        </p>
      )}
      <h3 className="text-sm font-medium">Create a branch at the current commit</h3>
      <p className="text-xs leading-5 text-muted">
        Current commit: {checkout.target.head ?? "No commit"}. Your working files and staged changes
        stay in this checkout.
      </p>
      <TextField>
        <Label>Branch name</Label>
        <Input
          required
          value={name}
          onChange={(event) => {
            setName(event.target.value);
          }}
          placeholder="feat/your-change"
        />
      </TextField>
      <div className="flex gap-2">
        <Button
          type="submit"
          size="sm"
          isDisabled={disabled || !name.trim() || checkout.target.head === null}
        >
          Create branch
        </Button>
        <Button size="sm" variant="tertiary" onPress={close}>
          Cancel
        </Button>
      </div>
    </form>
  );
}

export function SyncPanel(props: ActionProps) {
  const { submit, error } = useSubmit(props);
  const { checkout, disabled } = props;
  const [remote, setRemote] = useState("");
  const [destination, setDestination] = useState("");
  const selectedRemote =
    checkout.remotes.state === "ready"
      ? checkout.remotes.value.find((entry) => entry.name === remote)
      : undefined;
  if (checkout.remotes.state === "unavailable") {
    return (
      <p role="alert" className="text-sm text-danger">
        Remotes unavailable: {checkout.remotes.error}
      </p>
    );
  }

  return (
    <div className="grid max-w-lg gap-3">
      {error === null ? null : (
        <p role="alert" className="text-sm text-danger">
          {error}
        </p>
      )}
      <h3 className="text-sm font-medium">Fetch / Push</h3>
      <label className="grid gap-2 text-xs">
        Remote
        <select
          value={remote}
          onChange={(event) => {
            setRemote(event.target.value);
          }}
          className="h-10 rounded-lg border border-border bg-default px-3 text-sm"
        >
          <option value="">Select a remote</option>
          {checkout.remotes.value.map((entry) => (
            <option key={entry.name} value={entry.name}>
              {entry.name}
            </option>
          ))}
        </select>
      </label>
      {selectedRemote === undefined ? null : (
        <div className="text-xs leading-5 break-all text-muted">
          <p>Fetch: {selectedRemote.fetch_urls.join(", ") || "No URL"}</p>
          <p>Push: {selectedRemote.push_urls.join(", ") || "No URL"}</p>
        </div>
      )}
      <Button
        size="sm"
        isDisabled={disabled || remote === ""}
        onPress={() => {
          submit({ kind: "fetch", remote });
        }}
      >
        Fetch selected remote
      </Button>
      <div className="mt-2 border-t border-border pt-4">
        <p className="mb-3 text-xs leading-5 text-muted">
          Push source: {checkout.target.head ?? "No commit"}. Review the destination before sending
          this commit.
        </p>
        <TextField>
          <Label>Destination branch ref</Label>
          <Input
            value={destination}
            onChange={(event) => {
              setDestination(event.target.value);
            }}
            placeholder="refs/heads/feat/your-change"
          />
        </TextField>
        <Button
          size="sm"
          className="mt-3"
          isDisabled={
            disabled ||
            remote === "" ||
            !destination.startsWith("refs/heads/") ||
            checkout.target.head === null ||
            selectedRemote?.push_urls.length !== 1
          }
          onPress={() => {
            const source = checkout.target.head;
            if (source !== null) {
              submit({
                kind: "push",
                remote,
                source_oid: source,
                destination_ref: destination,
              });
            }
          }}
        >
          Push to selected destination
        </Button>
        {selectedRemote !== undefined && selectedRemote.push_urls.length !== 1 ? (
          <p className="mt-2 text-xs text-muted">
            Push requires one explicit destination URL. Resolve this remote configuration before
            pushing.
          </p>
        ) : null}
      </div>
    </div>
  );
}

function stagedFileKey(entry: { path: string }) {
  return entry.path;
}

export function CommitPanel(props: ActionProps) {
  const { submit, error } = useSubmit(props);
  const { checkout, disabled, close } = props;
  const [message, setMessage] = useState("");
  const [reviewed, setReviewed] = useState(false);
  const staged = useMemo(
    () =>
      checkout.status.entries.filter(
        (entry) => entry.index !== "." && entry.index !== " " && entry.kind === "tracked",
      ),
    [checkout.status.entries],
  );
  return (
    <form
      className="grid max-w-xl gap-3"
      onSubmit={(event) => {
        event.preventDefault();
        submit({ kind: "commit", message, reviewed_index: checkout.target.index_fingerprint });
      }}
    >
      {error === null ? null : (
        <p role="alert" className="text-sm text-danger">
          {error}
        </p>
      )}
      <h3 className="text-sm font-medium">Review the complete staged snapshot</h3>
      {props.commitGuidance.trim() === "" ? null : (
        <p className="text-xs leading-5 text-muted">
          Project commit guidance: {props.commitGuidance}
        </p>
      )}
      <p className="text-xs leading-5 text-muted">
        This commit includes every staged change listed here. Unstaged files remain in the checkout.
        Git hooks and signing run on the execution node.
      </p>
      <VirtualList
        items={staged}
        getItemKey={stagedFileKey}
        label={`${staged.length} staged files in this commit`}
        estimateSize={24}
        className="h-32"
        renderItem={(entry) => (
          <span className="block truncate text-xs leading-6" title={entry.path}>
            {entry.path}
          </span>
        )}
      />
      <TextField>
        <Label>Commit message</Label>
        <TextArea
          required
          rows={3}
          value={message}
          onChange={(event) => {
            setMessage(event.target.value);
          }}
        />
      </TextField>
      <label className="flex items-start gap-2 text-xs leading-5">
        <input
          type="checkbox"
          checked={reviewed}
          onChange={(event) => {
            setReviewed(event.target.checked);
          }}
          className="mt-1 accent-foreground"
        />
        I reviewed all staged files and the diff, including any truncated or binary content.
      </label>
      <div className="flex gap-2">
        <Button
          type="submit"
          size="sm"
          isDisabled={disabled || !reviewed || staged.length === 0 || !message.trim()}
        >
          Commit staged changes
        </Button>
        <Button size="sm" variant="tertiary" onPress={close}>
          Cancel
        </Button>
      </div>
    </form>
  );
}
