import type { ReactNode } from "react";

import { Badge } from "../../atoms/badge";
import { Button } from "../../atoms/button";
import { Icon } from "../../atoms/icon";
import {
  SelectContent,
  SelectItem,
  SelectRoot,
  SelectTrigger,
  SelectValue,
} from "../../atoms/select";
import { cn } from "../../classes";
import { AssistantPanel, AssistantProgress, PanelHeading } from "./shared";

export type VoiceState =
  | "disconnected"
  | "connecting"
  | "listening"
  | "thinking"
  | "speaking"
  | "muted";

/** Voice visualization receives real call state; it never opens microphones or a connection. */
export function VoiceOrb({
  state,
  label,
  amplitude = 0,
  connectLabel,
  disconnectLabel,
  onConnect,
  onDisconnect,
}: {
  state: VoiceState;
  label: string;
  amplitude?: number | undefined;
  connectLabel?: string;
  disconnectLabel?: string;
  onConnect?: () => void;
  onDisconnect?: () => void;
}) {
  const level = Number.isFinite(amplitude) ? Math.min(1, Math.max(0, amplitude)) : 0;
  const active = state === "listening" || state === "speaking";
  return (
    <figure aria-label={label} className="flex flex-col items-center gap-2">
      <svg
        viewBox="0 0 160 160"
        aria-hidden="true"
        className="size-40 rounded-full bg-primary-soft"
      >
        <circle
          cx="80"
          cy="80"
          r={active ? 48 * (0.72 + level * 0.28) : 30}
          className="fill-primary/10 transition-all duration-200 ease-out motion-reduce:transition-none"
        />
        <circle
          cx="80"
          cy="80"
          r={active ? 34 * (0.8 + level * 0.22) : 24}
          className="fill-primary/20 transition-all duration-150 ease-out motion-reduce:transition-none"
        />
        <circle
          cx="80"
          cy="80"
          r={active ? 20 * (0.9 + level * 0.2) : 17}
          className={cn(
            "fill-primary transition-all duration-150 ease-out motion-reduce:transition-none",
            (state === "thinking" || state === "connecting") && "motion-safe:animate-pulse",
            state === "muted" && "opacity-40",
          )}
        />
      </svg>
      {state === "disconnected" && onConnect !== undefined ? (
        <Button onClick={onConnect}>{connectLabel}</Button>
      ) : null}
      {state !== "disconnected" && onDisconnect !== undefined ? (
        <Button variant="ghost" onClick={onDisconnect}>
          {disconnectLabel}
        </Button>
      ) : null}
    </figure>
  );
}

export function VoiceConversation({
  state,
  label,
  transcript,
  muted,
  muteLabel,
  endLabel,
  onMuteChange,
  onEnd,
  amplitude,
  turns,
  hint,
  onInterrupt,
  interruptLabel,
}: {
  state: VoiceState;
  label: string;
  transcript: ReactNode;
  amplitude?: number;
  turns?: readonly { id: string; role: "user" | "assistant"; roleLabel: string; text: string }[];
  hint?: string;
  onInterrupt?: () => void;
  interruptLabel?: string;
  muted: boolean;
  muteLabel: string;
  endLabel: string;
  onMuteChange: (value: boolean) => void;
  onEnd: () => void;
}) {
  return (
    <AssistantPanel>
      <div className="flex flex-col items-center gap-3">
        <VoiceOrb state={muted ? "muted" : state} label={label} amplitude={muted ? 0 : amplitude} />
        <Badge tone="agent">{label}</Badge>
        <p className="text-xs text-text-muted">{hint}</p>
        {turns?.slice(-2).map((turn) => (
          <p key={turn.id} className="w-full text-ui motion-safe:animate-assistant-tool-step">
            <span className={turn.role === "assistant" ? "text-primary" : "text-text-muted"}>
              {turn.roleLabel}:{" "}
            </span>
            {turn.text}
          </p>
        ))}
        <div className="max-h-56 w-full overflow-y-auto text-body leading-body">{transcript}</div>
        <div className="flex flex-wrap gap-2">
          {state === "speaking" && onInterrupt !== undefined ? (
            <Button variant="ghost" onClick={onInterrupt}>
              {interruptLabel}
            </Button>
          ) : null}
          <Button
            variant="secondary"
            aria-pressed={muted}
            onClick={() => {
              onMuteChange(!muted);
            }}
          >
            <Icon name="microphone" />
            {muteLabel}
          </Button>
          <Button variant="destructive" onClick={onEnd}>
            {endLabel}
          </Button>
        </div>
      </div>
    </AssistantPanel>
  );
}

export function ReadAloud({
  label,
  words,
  activeIndex,
  playing,
  speed,
  playLabel,
  pauseLabel,
  speedLabel,
  onPlayChange,
  onSpeedChange,
  elapsed,
  duration,
  progressLabel,
}: {
  label: string;
  words: readonly { id: string; text: string }[];
  activeIndex: number;
  playing: boolean;
  speed: string;
  playLabel: string;
  pauseLabel: string;
  speedLabel: string;
  onPlayChange: (playing: boolean) => void;
  onSpeedChange: (speed: string) => void;
  elapsed?: string;
  duration?: string;
  progressLabel?: string;
}) {
  return (
    <AssistantPanel>
      <PanelHeading>{label}</PanelHeading>
      <p className="mb-4 text-body leading-body">
        {words.map((word, index) => (
          <span
            key={word.id}
            className={cn(
              "transition-colors duration-200 motion-reduce:transition-none",
              index < activeIndex && "text-text-muted",
              index === activeIndex && "rounded-pill bg-primary-soft text-primary",
              index > activeIndex && "text-ink",
            )}
          >
            {word.text}{" "}
          </span>
        ))}
      </p>
      <AssistantProgress
        label={progressLabel ?? label}
        value={Math.max(0, activeIndex)}
        max={Math.max(1, words.length)}
        className="duration-200 ease-linear"
      />
      <p className="text-xs text-text-muted tabular-nums">
        {elapsed} / {duration}
      </p>
      <div className="flex flex-wrap items-center gap-3">
        <Button
          variant="secondary"
          aria-pressed={playing}
          onClick={() => {
            onPlayChange(!playing);
          }}
        >
          {playing ? pauseLabel : playLabel}
        </Button>
        <div className="flex items-center gap-2 text-ui">
          <span>{speedLabel}</span>
          <SelectRoot value={speed} onValueChange={onSpeedChange}>
            <SelectTrigger aria-label={speedLabel}>
              <SelectValue>{speed}×</SelectValue>
            </SelectTrigger>
            <SelectContent>
              {["0.75", "1", "1.25", "1.5", "2"].map((value) => (
                <SelectItem key={value} value={value}>
                  {value}×
                </SelectItem>
              ))}
            </SelectContent>
          </SelectRoot>
        </div>
      </div>
    </AssistantPanel>
  );
}
