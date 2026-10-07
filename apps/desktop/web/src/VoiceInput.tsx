import { useEffect, useRef, useState } from "react";
import { Mic, Square } from "lucide-react";
import { Button } from "@/components/ui/button";

type Recognition = {
  lang: string;
  continuous: boolean;
  interimResults: boolean;
  onresult: ((event: { resultIndex: number; results: ArrayLike<{ isFinal: boolean; 0: { transcript: string } }> }) => void) | null;
  onerror: ((event: { error: string }) => void) | null;
  onend: (() => void) | null;
  start(): void;
  stop(): void;
  abort(): void;
};
type VoiceWindow = { SpeechRecognition?: new () => Recognition; webkitSpeechRecognition?: new () => Recognition };

export function VoiceInput({ disabled, onText, onError }: {
  disabled?: boolean;
  onText: (text: string) => void;
  onError: (error: string) => void;
}) {
  const Constructor = (window as unknown as VoiceWindow).SpeechRecognition ?? (window as unknown as VoiceWindow).webkitSpeechRecognition;
  const active = useRef<Recognition | null>(null);
  const callbacks = useRef({ onText, onError });
  callbacks.current = { onText, onError };
  const [listening, setListening] = useState(false);
  useEffect(() => () => {
    if (active.current) {
      active.current.onresult = active.current.onerror = active.current.onend = null;
      active.current.abort();
    }
  }, []);
  useEffect(() => { if (disabled) active.current?.abort(); }, [disabled]);
  const toggle = () => {
    if (active.current) { active.current.stop(); return; }
    if (!Constructor || disabled) return;
    const recognition = new Constructor();
    active.current = recognition;
    recognition.lang = navigator.language;
    recognition.continuous = true;
    recognition.interimResults = false;
    recognition.onresult = (event) => {
      const text = Array.from(event.results).slice(event.resultIndex).filter((result) => result.isFinal).map((result) => result[0].transcript).join(" ").trim();
      if (text) callbacks.current.onText(text);
    };
    recognition.onerror = ({ error }) => {
      if (error !== "aborted" && error !== "no-speech") callbacks.current.onError(error === "not-allowed" ? "Microphone access was denied. Allow microphone access and try again." : `Voice dictation failed (${error}). Try again or type your message.`);
    };
    recognition.onend = () => { active.current = null; setListening(false); };
    try { recognition.start(); setListening(true); }
    catch { active.current = null; callbacks.current.onError("Voice dictation could not start. Try again or type your message."); }
  };
  return <Button variant="ghost" size="icon-sm" className="composer-microphone rounded-full" aria-label={listening ? "Stop dictation" : "Dictate message"} aria-pressed={listening} disabled={disabled || !Constructor}
    title={Constructor ? listening ? "Stop dictation" : "Dictate message" : "Voice dictation is unavailable in this browser"} onClick={toggle}>{listening ? <Square size={14} /> : <Mic size={16} />}</Button>;
}
