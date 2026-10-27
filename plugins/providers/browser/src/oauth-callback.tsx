import { Button, Input, Label } from "@swarmie/ui-web";
import { useId, useRef, useState } from "react";

/** Callback URLs remain in an uncontrolled field and are cleared before forwarding to the node. */
export function OAuthCallback({
  callbackUri,
  onComplete,
}: {
  readonly callbackUri: string;
  readonly onComplete: (redirectUrl: string) => Promise<void>;
}) {
  const id = useId();
  const input = useRef<HTMLInputElement | null>(null);
  const [busy, setBusy] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);
  const [submitted, setSubmitted] = useState(false);
  function failed(error: unknown) {
    setFailure(error instanceof Error ? error.message : String(error));
  }
  async function complete() {
    const redirectUrl = input.current?.value.trim() ?? "";
    if (input.current !== null) {
      input.current.value = "";
    }
    setBusy(true);
    setFailure(null);
    setSubmitted(false);
    try {
      await onComplete(redirectUrl);
      setSubmitted(true);
    } catch (error) {
      failed(error);
    } finally {
      setBusy(false);
    }
  }
  return (
    <details className="grid gap-3 border-t border-border pt-3">
      <summary className="cursor-pointer text-sm text-muted">Paste callback URL</summary>
      <form
        className="mt-3 grid gap-3"
        onSubmit={(event) => {
          event.preventDefault();
          complete().catch(failed);
        }}
      >
        <p className="text-xs leading-relaxed text-muted">
          On the same computer, sign-in returns automatically. If your execution node runs on
          another computer, copy the complete URL after sign-in and paste it here, even if that page
          could not load.
        </p>
        <Label htmlFor={id}>Complete callback URL</Label>
        <Input
          id={id}
          ref={input}
          type="url"
          required
          autoComplete="off"
          autoCapitalize="none"
          spellCheck={false}
          disabled={busy}
          placeholder={callbackUri}
          maxLength={16_384}
        />
        <div>
          <Button type="submit" shape="panel" variant="secondary" loading={busy} isDisabled={busy}>
            Complete sign-in
          </Button>
        </div>
        {submitted ? (
          <output className="text-xs text-muted">Callback received. Finishing sign-in.</output>
        ) : null}
        {failure === null ? null : (
          <p role="alert" className="text-caption text-danger">
            {failure}
          </p>
        )}
      </form>
    </details>
  );
}
