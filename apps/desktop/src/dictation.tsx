import { Button, Input } from "@swarmie/ui-web";
import { useState } from "react";

import { recognitionConstructor, startBrowserSpeech } from "~/src/browser-speech";
import type { ClientExtension, ContributionRegistry, ScopedClientApi } from "~/src/contributions";
import { nonEmptyText, hasText } from "~/src/value-presence";

export const DICTATION_EXTENSION_ID = "@swarmie.dictation";
export type DictationSession = { stop: () => void; abort: () => void };
/** Providers capture on this client; implementations may transcribe locally or remotely. */
export type TranscriptionProvider = {
  id: string;
  available: () => boolean;
  start: (options: {
    language: string;
    text: (text: string) => void;
    error: (message: string) => void;
    ended: () => void;
  }) => DictationSession;
};
export const browserTranscription: TranscriptionProvider = {
  id: "browser-speech",
  available: () => recognitionConstructor() !== undefined,
  start: startBrowserSpeech,
};
function language() {
  try {
    return nonEmptyText(localStorage.getItem("swarmie.dictation.language")) ?? navigator.language;
  } catch {
    return navigator.language;
  }
}
export function createDictationExtension(
  registry: ContributionRegistry,
  onError: (message: string) => void,
  provider: TranscriptionProvider = browserTranscription,
): ClientExtension {
  let session: DictationSession | undefined;
  let state: "stopped" | "recording" | "processing" = "stopped";
  let serial = 0;
  const getState = () => state;
  let finish: (() => void) | undefined;
  const publish = () => {
    registry.setCommandEnabled(
      `${DICTATION_EXTENSION_ID}/toggle`,
      provider.available() && state !== "processing",
    );
    registry.update(DICTATION_EXTENSION_ID, "microphone", {
      icon: (() => {
        if (state === "recording") {
          return "stop";
        }
        if (state === "processing") {
          return "spinner";
        }
        return "microphone";
      })(),
      title: (() => {
        if (state === "recording") {
          return "Stop dictation";
        }
        if (state === "processing") {
          return "Finishing dictation";
        }
        return "Dictate message";
      })(),
      enabled: provider.available() && state !== "processing",
    });
    registry.update(DICTATION_EXTENSION_ID, "status", {
      text: (() => {
        if (state === "stopped") {
          return "";
        }
        if (state === "recording") {
          return "Recording dictation…";
        }
        return "Processing dictation…";
      })(),
    });
  };
  const cancel = () => {
    serial += 1;
    session?.abort();
    session = undefined;
    state = "stopped";
    finish?.();
    finish = undefined;
    publish();
  };
  return {
    id: DICTATION_EXTENSION_ID,
    manifest: {
      version: 1,
      commands: [
        {
          id: "toggle",
          title: "Dictate message",
          when: ["composerReady"],
          shortcut: "Mod+Shift+d",
        },
        { id: "settings", title: "Dictation settings", when: [] },
      ],
      slots: [
        {
          id: "microphone",
          icon: "microphone",
          title: "Dictate message",
          slot: "composer.actions",
          kind: "action",
          command: "toggle",
          order: 0,
          when: [],
          enabledWhen: ["composerReady"],
          clients: [],
        },
        {
          id: "status",
          title: "Dictation status",
          text: "",
          slot: "composer.status",
          kind: "text",
          order: 0,
          when: [],
          enabledWhen: [],
          clients: [],
        },
        {
          id: "settings",
          title: "Dictation",
          slot: "settings.sections",
          kind: "section",
          order: 0,
          when: [],
          enabledWhen: [],
          clients: [],
        },
      ],
      screens: [{ id: "settings", title: "Dictation settings", clients: [] }],
      panels: [],
    },
    handlers: {
      settings: (api) => {
        api.openScreen("settings");
      },
      toggle: async (api) => {
        if (state === "recording") {
          state = "processing";
          publish();
          try {
            session?.stop();
          } catch {
            cancel();
            onError("Voice dictation could not stop. Try again or type your message.");
          }
          return;
        }
        if (state !== "stopped" || !provider.available()) {
          return;
        }
        // Native capture emits many transcripts, but completion/cancellation settles this lifetime once.
        // oxlint-disable-next-line promise/avoid-new
        await new Promise<void>((resolve) => {
          finish = resolve;
          serial += 1;
          const current = serial;
          state = "recording";
          publish();
          api.signal.addEventListener("abort", cancel, { once: true });
          try {
            const started = provider.start({
              language: language(),
              text: (text) => {
                if (current === serial && !api.signal.aborted) {
                  try {
                    api.insertText(text, { separateWords: true });
                  } catch {
                    cancel();
                  }
                }
              },
              error: (error) => {
                if (current === serial && !api.signal.aborted) {
                  onError(error);
                  cancel();
                }
              },
              ended: () => {
                if (current === serial) {
                  session = undefined;
                  state = "stopped";
                  api.signal.removeEventListener("abort", cancel);
                  finish?.();
                  finish = undefined;
                  publish();
                }
              },
            });
            if (current === serial && getState() !== "stopped") {
              session = started;
            } else {
              started.abort();
            }
          } catch {
            cancel();
            onError("Voice dictation could not start. Try again or type your message.");
          }
        });
      },
    },
    activate: ({ on }) => {
      publish();
      const off = on("context.changed", (event) => {
        if (!event.context.composerReady) {
          cancel();
        }
      });
      return () => {
        off();
        cancel();
      };
    },
  };
}
export function DictationSettings({ api }: { api: ScopedClientApi }) {
  const [value, setValue] = useState(language);
  const [saved, setSaved] = useState(false);
  const [error, setError] = useState("");
  return (
    <div className="dictation-settings">
      <p>
        Capture speech on the client where you use Swarmie. Dictation works independently of the
        selected harness.
      </p>
      <p>
        The built-in browser speech provider may send audio to a remote service. Local processing is
        not guaranteed. Availability depends on browser support and microphone permission.
      </p>
      <label htmlFor="dictation-language">Recognition language</label>
      <Input
        id="dictation-language"
        value={value}
        onChange={(event) => {
          setValue(event.target.value);
          setSaved(false);
        }}
        placeholder="en-US"
      />
      <Button
        variant="secondary"
        onClick={() => {
          try {
            Intl.getCanonicalLocales(value);
            localStorage.setItem("swarmie.dictation.language", value);
            setSaved(true);
            setError("");
          } catch {
            setError("Enter a valid language tag and allow this client to save settings.");
          }
        }}
      >
        Save language
      </Button>
      {saved && <p aria-live="polite">Language saved. It applies to the next recording.</p>}
      {error && <p role="alert">{error}</p>}
      <p>
        Use the microphone, the “Dictate message” command, or Mod+Shift+D. Switching conversations
        or nodes stops capture and discards late callbacks.
      </p>
      <p className="muted">
        Conversation scope:{" "}
        {hasText(api.context.conversationId) ? "Current conversation" : "No conversation selected"}
      </p>
    </div>
  );
}
