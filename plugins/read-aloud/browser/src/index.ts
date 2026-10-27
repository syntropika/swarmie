import { conversationsOrb } from "@swarmie/plugin-conversations-browser";
import type { ConversationsUiApi } from "@swarmie/plugin-conversations-browser";
import { orb } from "@syntropika/tembra-sdk";
import { Mic, Volume2 } from "lucide-react";

import { recognitionConstructor, startDictation } from "./dictation";
import { spokenText } from "./spoken-text";

/** Contributions are registered once at activation and removed by Tembra resource cleanup. */
export function readAloudOrb(language: () => string) {
  return orb({
    name: "swarmie.read-aloud.ui",
    apiVersion: 1,
    requires: {
      conversations: {
        orb: "swarmie.conversations.ui",
        apiVersion: 1,
        services: { "swarmie.conversations.ui": [] },
      },
    },
    services: {
      "swarmie.read-aloud.ui": {
        create(self) {
          const owner = self.get(conversationsOrb.services["swarmie.conversations.ui"], "main");
          self.resource("contributions", {
            deps: [owner],
            open: () => {
              const api = owner.opaque<ConversationsUiApi>("ui");
              let recording: ReturnType<typeof startDictation> | null = null;
              const disposers = [
                api.composer.registerAction({
                  id: "swarmie.read-aloud.dictate",
                  owner: "swarmie.read-aloud",
                  label: "Dictate message",
                  icon: Mic,
                  slot: "before-send",
                  order: 50,
                  execute(context) {
                    if (recording !== null) {
                      recording.dispose();
                      recording = null;
                      return;
                    }
                    const Constructor = recognitionConstructor();
                    if (Constructor === undefined) {
                      throw new Error("Speech recognition is unavailable in this browser.");
                    }
                    recording = startDictation(Constructor, {
                      language: language(),
                      insertText: (text) => {
                        if (!context.signal.aborted) {
                          context.insertText(text);
                        }
                      },
                      onListening: () => undefined,
                      onError: (failure) => {
                        console.error(failure);
                      },
                    });
                    context.signal.addEventListener(
                      "abort",
                      () => {
                        recording?.dispose();
                        recording = null;
                      },
                      { once: true },
                    );
                  },
                }),
                api.messages.registerAction({
                  id: "swarmie.read-aloud.speak",
                  owner: "swarmie.read-aloud",
                  label: "Read aloud",
                  icon: Volume2,
                  roles: ["assistant"],
                  order: 20,
                  execute({ message }) {
                    if (!("speechSynthesis" in globalThis)) {
                      throw new Error("Speech synthesis is unavailable in this browser.");
                    }
                    speechSynthesis.cancel();
                    const text = spokenText(message.text);
                    if (text !== "") {
                      const utterance = new SpeechSynthesisUtterance(text);
                      utterance.lang = language();
                      speechSynthesis.speak(utterance);
                    }
                  },
                }),
              ];
              return () => {
                recording?.dispose();
                if ("speechSynthesis" in globalThis) {
                  speechSynthesis.cancel();
                }
                for (const dispose of disposers) {
                  dispose();
                }
              };
            },
            close: (dispose) => {
              dispose();
            },
          });
          return self.state({ available: true });
        },
        methods: {},
      },
    },
  });
}
