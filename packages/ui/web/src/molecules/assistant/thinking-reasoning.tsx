import { useEffect, useId, useRef, useState } from "react";
import type { ComponentPropsWithRef } from "react";

import { Button } from "../../atoms/button";
import { Icon } from "../../atoms/icon";
import { cn } from "../../classes";
import { ThinkingState } from "./thinking-state";

export type ReasoningSentence = { id: string; text: string };

/** Host-driven reasoning stream; manual disclosure and reader scroll take precedence over streaming. */
export function ThinkingReasoning({
  sentences,
  phase,
  thinkingLabel,
  completedLabel,
  toggleLabel,
  open: controlledOpen,
  defaultOpen = false,
  onOpenChange,
  className,
  ...props
}: ComponentPropsWithRef<"section"> & {
  sentences: readonly ReasoningSentence[];
  phase: "thinking" | "completed";
  thinkingLabel: string;
  completedLabel: string;
  toggleLabel: string;
  open?: boolean;
  defaultOpen?: boolean;
  onOpenChange?: (open: boolean) => void;
}) {
  const id = useId();
  const [userOpen, setUserOpen] = useState<boolean | null>(null);
  const [fades, setFades] = useState({ top: false, bottom: false });
  const viewport = useRef<HTMLDivElement>(null);
  const stream = useRef<HTMLDivElement>(null);
  const completed = phase === "completed";
  const expanded = controlledOpen ?? userOpen ?? (!completed || defaultOpen);

  useEffect(() => {
    const view = viewport.current;
    const track = stream.current;
    if (!view || !track || !expanded) {
      return undefined;
    }
    let pinned = true;
    let lastScrollTop = view.scrollTop;
    let lastScrollHeight = view.scrollHeight;
    const updateFades = () => {
      setFades({
        top: view.scrollTop > 1,
        bottom: view.scrollTop + view.clientHeight < view.scrollHeight - 1,
      });
    };
    const pin = () => {
      if (!completed && pinned) {
        view.scrollTop = view.scrollHeight;
      }
      updateFades();
    };
    const onScroll = () => {
      const atBottom = view.scrollHeight - view.scrollTop - view.clientHeight <= 1;
      if (atBottom) {
        pinned = true;
      } else if (view.scrollTop < lastScrollTop && view.scrollHeight === lastScrollHeight) {
        pinned = false;
      }
      lastScrollTop = view.scrollTop;
      lastScrollHeight = view.scrollHeight;
      updateFades();
    };
    pin();
    view.addEventListener("scroll", onScroll);
    const observer = new ResizeObserver(pin);
    observer.observe(track);
    return () => {
      view.removeEventListener("scroll", onScroll);
      observer.disconnect();
    };
  }, [completed, expanded]);

  return (
    <section {...props} className={cn("flex w-90 max-w-full flex-col", className)}>
      <Button
        variant="ghost"
        size="menu"
        className="self-start px-2.5 text-text-muted"
        aria-label={toggleLabel}
        aria-expanded={expanded}
        aria-controls={id}
        onClick={() => {
          if (controlledOpen === undefined) {
            setUserOpen(!expanded);
          }
          onOpenChange?.(!expanded);
        }}
      >
        {completed ? completedLabel : <ThinkingState>{thinkingLabel}</ThinkingState>}
        <Icon
          name="chevron"
          className={cn(
            "size-3 transition-transform duration-200 ease-assistant-disclosure motion-reduce:transition-none",
            expanded && "rotate-180",
          )}
        />
      </Button>
      <div
        id={id}
        aria-hidden={!expanded}
        aria-busy={!completed}
        inert={!expanded}
        className={cn(
          "grid transition-all duration-200 ease-assistant-disclosure motion-reduce:transition-none",
          expanded ? "grid-rows-[1fr]" : "grid-rows-[0fr]",
        )}
      >
        <div className="min-h-0 overflow-hidden">
          <section
            ref={viewport}
            aria-label={toggleLabel}
            // oxlint-disable-next-line jsx-a11y/no-noninteractive-tabindex -- Keyboard users need focus to scroll this bounded reasoning region.
            tabIndex={0}
            className={cn(
              "max-h-64 [scrollbar-width:none] overflow-y-auto py-2 pr-2 pl-2.5 text-ui leading-content text-text-muted transition-all duration-200 ease-assistant-disclosure focus-visible:outline-2 focus-visible:outline-primary motion-reduce:transition-none [&::-webkit-scrollbar]:hidden",
              expanded ? "translate-y-0 opacity-100 blur-none" : "-translate-y-4 opacity-0 blur-xs",
              fades.top && fades.bottom && "assistant-fade-both",
              fades.top && !fades.bottom && "assistant-fade-top",
              !fades.top && fades.bottom && "assistant-fade-bottom",
            )}
          >
            <div ref={stream} className="space-y-3">
              {sentences.map((sentence) => (
                <p key={sentence.id} className="motion-safe:animate-assistant-sentence-in">
                  {sentence.text}
                </p>
              ))}
            </div>
          </section>
        </div>
      </div>
    </section>
  );
}
