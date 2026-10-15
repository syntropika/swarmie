import type { ReactNode } from "react";

import { Button } from "../../atoms/button";
import { Icon } from "../../atoms/icon";
import { cn } from "../../classes";
import { AlertContent, AlertDescription, AlertIcon, AlertRoot, AlertTitle } from "../alert";
import { ThinkingState } from "./thinking-state";

/** Quiet waiting state. Its caller controls when work starts and stops. */
export function AssistantLoader({ label, tick }: { label: string; tick: number }) {
  const offset = Math.floor(tick / 3);
  return (
    <output className="inline-flex flex-col items-center gap-3">
      <span aria-hidden="true" className="grid grid-cols-3 gap-1">
        {[0, 1, 2, 3, 4, 5, 6, 7, 8].map((cell) => (
          <span
            key={cell}
            className={cn(
              "size-2 rounded-full bg-ink transition-opacity duration-300 motion-reduce:transition-none",
              (cell * 2 + offset) % 9 < 3 ? "opacity-90" : "opacity-15",
            )}
          />
        ))}
      </span>
      <ThinkingState>{label}</ThinkingState>
    </output>
  );
}

export function TypingIndicator({ label }: { label: string }) {
  return (
    <output className="inline-flex items-center gap-1.5">
      <span className="sr-only">{label}</span>
      {[
        "motion-safe:animate-assistant-typing-first",
        "motion-safe:animate-assistant-typing-second",
        "motion-safe:animate-assistant-typing",
      ].map((dot) => (
        <span
          key={dot}
          aria-hidden="true"
          className={cn("size-1.5 rounded-full bg-text-muted", dot)}
        />
      ))}
    </output>
  );
}

export function GuardrailNotice({
  title,
  children,
  alternative,
  onAlternative,
  policy,
  alternatives,
  onPick,
}: {
  title: string;
  children: ReactNode;
  alternative?: string;
  onAlternative?: () => void;
  policy?: string;
  alternatives?: readonly string[];
  onPick?: (alternative: string) => void;
}) {
  return (
    <AlertRoot tone="info">
      <AlertIcon>
        <Icon name="lock" />
      </AlertIcon>
      <AlertContent>
        <AlertTitle>{title}</AlertTitle>
        {policy === undefined ? null : <p className="text-xs text-text-muted">{policy}</p>}
        <AlertDescription>{children}</AlertDescription>
        {alternatives?.map((item) =>
          onPick ? (
            <Button
              key={item}
              variant="secondary"
              className="mt-3"
              onClick={() => {
                onPick(item);
              }}
            >
              {item}
            </Button>
          ) : (
            <p key={item} className="mt-3 text-ui">
              {item}
            </p>
          ),
        )}
        {alternative !== undefined && onAlternative ? (
          <Button variant="secondary" className="mt-3" onClick={onAlternative}>
            {alternative}
          </Button>
        ) : null}
      </AlertContent>
    </AlertRoot>
  );
}
