import type { Message } from "@swarmie/plugin-conversations-browser";
import { Button, Disclosure } from "@swarmie/ui-web";

/** Only settled result records are available; no name or success state is inferred. */
export function ToolResults({ messages }: { messages: readonly Message[] }) {
  return (
    <Disclosure>
      <Disclosure.Heading>
        <Button slot="trigger" variant="ghost" size="sm">
          <Disclosure.Indicator />
          {messages.length === 1 ? "Tool result" : `Tool results (${messages.length})`}
        </Button>
      </Disclosure.Heading>
      <Disclosure.Content>
        <Disclosure.Body>
          <div className="grid min-w-0 gap-3">
            {messages.map((message) => (
              <div key={message.id} className="min-w-0">
                <pre className="max-h-64 overflow-auto rounded-group bg-group p-3 text-caption break-words whitespace-pre-wrap">
                  {message.text.slice(0, 8192)}
                </pre>
                {message.truncated === true || message.text.length > 8192 ? (
                  <p className="mt-2 text-caption text-muted">
                    This preview is shortened. The full result remains in the conversation history.
                  </p>
                ) : null}
              </div>
            ))}
          </div>
        </Disclosure.Body>
      </Disclosure.Content>
    </Disclosure>
  );
}
