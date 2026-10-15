import { Mic, Square } from "lucide-react";
import { useEffect, useRef, useState } from "react";

import { Button } from "~/src/components/ui/button";
import { browserTranscription } from "~/src/dictation";
import type { DictationSession } from "~/src/dictation";

type VoiceProps = {
  disabled?: boolean;
  onText: (text: string) => void;
  onError: (error: string) => void;
};
/** Dictation remains owned by the client that displays this control. */
export function VoiceInput(props: VoiceProps) {
  return <VoiceSession key={String(props.disabled === true)} {...props} />;
}
function VoiceSession({ disabled, onText, onError }: VoiceProps) {
  const active = useRef<DictationSession | null>(null);
  const callbacks = useRef({ onText, onError });
  useEffect(() => {
    callbacks.current = { onText, onError };
  }, [onText, onError]);
  const [listening, setListening] = useState(false);
  const available = browserTranscription.available();
  useEffect(
    () => () => {
      active.current?.abort();
      active.current = null;
    },
    [],
  );
  const toggle = () => {
    if (active.current) {
      active.current.stop();
      return;
    }
    if (!available || disabled === true) {
      return;
    }
    try {
      active.current = browserTranscription.start({
        language: navigator.language,
        text: (text) => {
          callbacks.current.onText(text);
        },
        error: (error) => {
          callbacks.current.onError(error);
        },
        ended: () => {
          active.current = null;
          setListening(false);
        },
      });
      setListening(true);
    } catch {
      callbacks.current.onError("Voice dictation could not start. Try again or type your message.");
    }
  };
  return (
    <Button
      variant="composer"
      shape="pill"
      size="icon-sm"
      aria-label={listening ? "Stop dictation" : "Dictate message"}
      aria-pressed={listening}
      disabled={disabled === true || !available}
      title={(() => {
        if (available) {
          if (listening) {
            return "Stop dictation";
          }
          return "Dictate message";
        }
        return "Voice dictation is unavailable in this browser";
      })()}
      onClick={toggle}
    >
      {listening ? <Square size={14} /> : <Mic size={16} />}
    </Button>
  );
}
