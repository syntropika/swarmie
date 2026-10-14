import { useState } from "react";
import type { ClientExtension, ContributionRegistry, ScopedClientApi } from "./contributions";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";

export type DictationSession = { stop(): void; abort(): void };
/** Providers capture on this client; implementations may transcribe locally or remotely. */
export type TranscriptionProvider = {
  id: string; available(): boolean;
  start(options: { language: string; text(text: string): void; error(message: string): void; ended(): void }): DictationSession;
};
type Recognition = {
  lang: string; continuous: boolean; interimResults: boolean;
  onresult: ((event: { resultIndex: number; results: ArrayLike<{ isFinal: boolean; 0: { transcript: string } }> }) => void) | null;
  onerror: ((event: { error: string }) => void) | null; onend: (() => void) | null;
  start(): void; stop(): void; abort(): void;
};
function recognitionConstructor() {
  const client = window as unknown as { SpeechRecognition?: new () => Recognition; webkitSpeechRecognition?: new () => Recognition };
  return client.SpeechRecognition ?? client.webkitSpeechRecognition;
}
export const browserTranscription: TranscriptionProvider = {
  id: "browser-speech", available: () => !!recognitionConstructor(),
  start(options) {
    const Constructor = recognitionConstructor();
    if (!Constructor) throw new Error("Voice dictation is unavailable in this browser.");
    const recognition = new Constructor();
    recognition.lang = options.language; recognition.continuous = true; recognition.interimResults = false;
    recognition.onresult = (event) => {
      const text = Array.from(event.results).slice(event.resultIndex).filter((result) => result.isFinal).map((result) => result[0].transcript).join(" ").trim();
      if (text) options.text(text);
    };
    recognition.onerror = ({ error }) => {
      if (error !== "aborted" && error !== "no-speech") options.error(error === "not-allowed" ? "Microphone access was denied. Allow microphone access and try again." : `Voice dictation failed (${error}). Try again or type your message.`);
    };
    recognition.onend = () => { detach(); options.ended(); };
    const detach = () => { recognition.onresult = recognition.onerror = recognition.onend = null; };
    try { recognition.start(); } catch (error) { detach(); throw error; }
    return { stop: () => recognition.stop(), abort: () => { detach(); recognition.abort(); } };
  },
};
function language() {
  try { return localStorage.getItem("swarmie.dictation.language") || navigator.language; }
  catch { return navigator.language; }
}
export function createDictationExtension(registry: ContributionRegistry, onError: (message: string) => void, provider: TranscriptionProvider = browserTranscription): ClientExtension {
  let session: DictationSession | undefined;
  let state: "stopped" | "recording" | "processing" = "stopped";
  let serial = 0;
  let finish: (() => void) | undefined;
  const publish = () => {
    registry.setCommandEnabled("dictation/toggle", provider.available() && state !== "processing");
    registry.update("dictation", "microphone", { icon: state === "recording" ? "stop" : state === "processing" ? "spinner" : "microphone", title: state === "recording" ? "Stop dictation" : state === "processing" ? "Finishing dictation" : "Dictate message", enabled: provider.available() && state !== "processing" });
    registry.update("dictation", "status", { text: state === "stopped" ? "" : state === "recording" ? "Recording dictation…" : "Processing dictation…" });
  };
  const cancel = () => { serial++; session?.abort(); session = undefined; state = "stopped"; finish?.(); finish = undefined; publish(); };
  return {
    id: "dictation",
    manifest: { version: 1, commands: [
      { id: "toggle", title: "Dictate message", when: ["composerReady"], shortcut: "Mod+Shift+d" },
      { id: "settings", title: "Dictation settings", when: [] },
    ], slots: [
      { id: "microphone", icon: "microphone", title: "Dictate message", slot: "composer.actions", kind: "action", command: "toggle", order: 0, when: [], enabledWhen: ["composerReady"], clients: [] },
      { id: "status", title: "Dictation status", text: "", slot: "composer.status", kind: "text", order: 0, when: [], enabledWhen: [], clients: [] },
      { id: "settings", title: "Dictation", slot: "settings.sections", kind: "section", order: 0, when: [], enabledWhen: [], clients: [] },
    ], screens: [{ id: "settings", title: "Dictation settings", clients: [] }], panels: [] },
    handlers: {
      settings: (api) => api.openScreen("settings"),
      toggle: (api) => {
        if (state === "recording") { state = "processing"; publish(); try { session?.stop(); } catch { cancel(); onError("Voice dictation could not stop. Try again or type your message."); } return; }
        if (state !== "stopped" || !provider.available()) return;
        return new Promise<void>((resolve) => {
          finish = resolve;
          const current = ++serial;
          state = "recording"; publish();
          api.signal.addEventListener("abort", cancel, { once: true });
          try {
            const started = provider.start({ language: language(),
              text: (text) => { if (current === serial && !api.signal.aborted) { try { api.insertText(text, { separateWords: true }); } catch { cancel(); } } },
              error: (error) => { if (current === serial && !api.signal.aborted) { onError(error); cancel(); } },
              ended: () => { if (current === serial) { session = undefined; state = "stopped"; api.signal.removeEventListener("abort", cancel); finish?.(); finish = undefined; publish(); } },
            });
            if (current === serial && (state as string) !== "stopped") session = started; else started.abort();
          } catch { cancel(); onError("Voice dictation could not start. Try again or type your message."); }
        });
      },
    },
    activate: ({ on }) => {
      publish();
      const off = on("context.changed", (event) => { if (!event.context.composerReady) cancel(); });
      return () => { off(); cancel(); };
    },
  };
}
export function DictationSettings({ api }: { api: ScopedClientApi }) {
  const [value, setValue] = useState(language);
  const [saved, setSaved] = useState(false);
  const [error, setError] = useState("");
  return <div className="dictation-settings">
    <p>Capture speech on the client where you use Swarmie. Dictation works independently of the selected harness.</p>
    <p>The built-in browser speech provider may send audio to a remote service. Local processing is not guaranteed. Availability depends on browser support and microphone permission.</p>
    <label htmlFor="dictation-language">Recognition language</label>
    <Input id="dictation-language" value={value} onChange={(event) => { setValue(event.target.value); setSaved(false); }} placeholder="en-US" />
    <Button variant="secondary" onClick={() => {
      try { Intl.getCanonicalLocales(value); localStorage.setItem("swarmie.dictation.language", value); setSaved(true); setError(""); }
      catch { setError("Enter a valid language tag and allow this client to save settings."); }
    }}>Save language</Button>
    {saved && <p role="status">Language saved. It applies to the next recording.</p>}
    {error && <p role="alert">{error}</p>}
    <p>Use the microphone, the “Dictate message” command, or Mod+Shift+D. Switching conversations or nodes stops capture and discards late callbacks.</p>
    <p className="muted">Conversation scope: {api.context.conversationId ? "Current conversation" : "No conversation selected"}</p>
  </div>;
}
