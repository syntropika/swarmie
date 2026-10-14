import { useEffect, useRef, useState } from "react";
import { Mic, Square } from "lucide-react";
import { Button } from "@/components/ui/button";
import { browserTranscription, type DictationSession } from "./dictation";

/** Compatibility component for clients embedding the original standalone control. */
export function VoiceInput({ disabled, onText, onError }: {
  disabled?: boolean; onText: (text: string) => void; onError: (error: string) => void;
}) {
  const active = useRef<DictationSession | null>(null);
  const callbacks = useRef({ onText, onError });
  callbacks.current = { onText, onError };
  const [listening, setListening] = useState(false);
  const available = browserTranscription.available();
  useEffect(() => () => { active.current?.abort(); active.current = null; }, []);
  useEffect(() => { if (disabled) { active.current?.abort(); active.current = null; setListening(false); } }, [disabled]);
  const toggle = () => {
    if (active.current) { active.current.stop(); return; }
    if (!available || disabled) return;
    try {
      active.current = browserTranscription.start({ language: navigator.language,
        text: (text) => callbacks.current.onText(text), error: (error) => callbacks.current.onError(error),
        ended: () => { active.current = null; setListening(false); },
      });
      setListening(true);
    } catch { callbacks.current.onError("Voice dictation could not start. Try again or type your message."); }
  };
  return <Button variant="ghost" size="icon-sm" className="composer-microphone rounded-full" aria-label={listening ? "Stop dictation" : "Dictate message"} aria-pressed={listening} disabled={disabled || !available}
    title={available ? listening ? "Stop dictation" : "Dictate message" : "Voice dictation is unavailable in this browser"} onClick={toggle}>{listening ? <Square size={14} /> : <Mic size={16} />}</Button>;
}
