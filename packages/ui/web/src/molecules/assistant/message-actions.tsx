import { createContext, useContext, useEffect, useId, useMemo, useRef, useState } from "react";
import type { ComponentPropsWithRef, ReactNode } from "react";

import { Button } from "../../atoms/button";
import { Icon } from "../../atoms/icon";
import type { IconName } from "../../atoms/icon";
import { Input } from "../../atoms/input";
import { cn } from "../../classes";
import {
  ActionPopoverContent,
  ActionPopoverItem,
  ActionPopoverRoot,
  ActionPopoverTrigger,
  useActionPopover,
} from "../action-popover";
import { MESSAGE_REACTION_GROUPS } from "./message-reactions";
import type { EmojiEntry, EmojiGroup } from "./message-reactions";

/** Feedback is controlled by the host and can be cleared by selecting the same vote again. */
export type MessageVote = "up" | "down" | null;

export type MessageActionsLabels = {
  good: string;
  bad: string;
  copy: string;
  copied: string;
  react: string;
  reaction: (emoji: string) => string;
  reply: string;
  more: string;
  aloud: string;
  fork: string;
  report: string;
  search: string;
  empty: string;
  back: string;
};

/** Execution and relative-time formatting belong to the consumer, keeping UI free of app workflows. */
export type MessageActionsProps = Omit<ComponentPropsWithRef<"fieldset">, "children"> & {
  label: string;
  labels: MessageActionsLabels;
  text: string;
  vote: MessageVote;
  onVote: (vote: MessageVote) => void;
  reaction: string | null;
  onReact: (emoji: string | null) => void;
  copied: boolean;
  onCopy: (text: string) => void;
  onReply: () => void;
  onAloud?: () => void;
  onFork?: () => void;
  onReport?: () => void;
  onRegenerate?: () => void;
  regenerating?: boolean;
  regenerateLabel?: string;
  regeneratingLabel?: string;
  disabled?: boolean;
  timestamp?: { dateTime: string; relative: string; absolute: string };
  reactionGroups?: readonly EmojiGroup[];
  quickReactions?: readonly EmojiEntry[];
};

const QUICK_REACTIONS: readonly EmojiEntry[] = [
  ["👍", "Thumbs up"],
  ["❤️", "Heart"],
  ["😄", "Smile"],
  ["🎉", "Celebrate"],
  ["🤔", "Thinking"],
  ["👀", "Eyes"],
];

type CueState = { anchor: string; label: string } | null;
type CueApi = {
  current: CueState;
  show: (anchor: string, label: string) => void;
  hide: () => void;
  id: string;
};
const CueContext = createContext<CueApi | null>(null);

/** One top-layer tooltip follows actions without being clipped by transcript overflow. */
function ActionCue({ label, children }: { label: string; children: ReactNode }) {
  const cue = useContext(CueContext);
  const anchor = useId();
  const ref = useRef<HTMLSpanElement>(null);
  const active = cue?.current?.anchor === anchor;
  useEffect(() => {
    const element = ref.current?.querySelector("button, time");
    if (element && cue) {
      if (active) {
        element.setAttribute("aria-describedby", cue.id);
      } else {
        element.removeAttribute("aria-describedby");
      }
    }
    return () => {
      element?.removeAttribute("aria-describedby");
    };
  }, [active, cue]);
  return (
    <span
      ref={ref}
      className={cn("inline-flex", active && "[anchor-name:--assistant-cue]")}
      onMouseEnter={() => {
        cue?.show(anchor, label);
      }}
      onFocus={() => {
        cue?.show(anchor, label);
      }}
    >
      {children}
    </span>
  );
}

function CueBoundary({ children }: { children: ReactNode }) {
  const ref = useRef<HTMLDivElement>(null);
  const cue = useContext(CueContext);
  useEffect(() => {
    const element = ref.current;
    if (!element || !cue) {
      return () => {
        // No listener was registered.
      };
    }
    const hide = () => {
      cue.hide();
    };
    const blur = (event: FocusEvent) => {
      if (!(event.relatedTarget instanceof Node) || !element.contains(event.relatedTarget)) {
        cue.hide();
      }
    };
    element.addEventListener("mouseleave", hide);
    element.addEventListener("focusout", blur);
    return () => {
      element.removeEventListener("mouseleave", hide);
      element.removeEventListener("focusout", blur);
    };
  }, [cue]);
  return (
    <div ref={ref} className="contents">
      {children}
    </div>
  );
}

function RowCue({ id, current }: { id: string; current: CueState }) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const element = ref.current;
    if (!element) {
      return;
    }
    if (current) {
      element.textContent = current.label;
    }
    if (current && !element.matches(":popover-open")) {
      element.showPopover();
    } else if (!current && element.matches(":popover-open")) {
      element.hidePopover();
    }
  }, [current]);
  return (
    <div
      ref={ref}
      id={id}
      role="tooltip"
      popover="manual"
      className="assistant-message-cue pointer-events-none fixed inset-auto m-0 mb-2 w-max max-w-[calc(100vw-24px)] rounded-pill border-0 bg-inverse px-2 py-1 text-ui whitespace-nowrap text-inverse-text [position-anchor:--assistant-cue] [position-area:top] [position-try-fallbacks:flip-block,flip-inline] motion-reduce:transition-none"
    />
  );
}

function leadingNumber(text: string) {
  return Number(/^\d+/u.exec(text)?.[0]);
}

/** Only changed characters roll; the full caller-formatted timestamp stays accessible. */
function AnimatedTime({ value }: { value: string }) {
  const [settled, setSettled] = useState(value);
  useEffect(() => {
    if (value === settled) {
      // No animation timer exists while the displayed label is already settled.
      return () => {
        // No timer was registered.
      };
    }
    const timer = window.setTimeout(() => {
      setSettled(value);
    }, 350);
    return () => {
      window.clearTimeout(timer);
    };
  }, [value, settled]);
  const down = leadingNumber(value) < leadingNumber(settled);
  const segmenter = new Intl.Segmenter(undefined, { granularity: "grapheme" });
  const characters = [...segmenter.segment(value)].map(
    ({ segment: character, index: position }) => ({
      key: `${value.slice(0, position)}-${character}-${position}`,
      character,
      previous: settled[position] ?? "",
    }),
  );
  return (
    <span className="inline-flex h-5 overflow-hidden">
      <span className="sr-only">{value}</span>
      <span aria-hidden="true" className="inline-flex motion-reduce:hidden">
        {characters.map(({ key, character, previous }) =>
          previous === character ? (
            <span key={key}>{character}</span>
          ) : (
            <span key={key} className="h-5 overflow-hidden">
              <span
                className={cn(
                  "flex flex-col",
                  down
                    ? "motion-safe:animate-assistant-roll-down350"
                    : "motion-safe:animate-assistant-roll",
                )}
              >
                <span className="h-5">{down ? character : previous}</span>
                <span className="h-5">{down ? previous : character}</span>
              </span>
            </span>
          ),
        )}
      </span>
      <span aria-hidden="true" className="hidden motion-reduce:inline">
        {value}
      </span>
    </span>
  );
}

function MoreActions({
  labels,
  onAloud,
  onFork,
  onReport,
}: {
  labels: MessageActionsLabels;
  onAloud: (() => void) | undefined;
  onFork: (() => void) | undefined;
  onReport: (() => void) | undefined;
}) {
  const { close } = useActionPopover();
  const actions: {
    label: string;
    icon: IconName;
    run: (() => void) | undefined;
    danger?: boolean;
  }[] = [
    { label: labels.aloud, icon: "volume", run: onAloud },
    { label: labels.fork, icon: "fork", run: onFork },
    { label: labels.report, icon: "flag", run: onReport, danger: true },
  ];
  return (
    <ActionPopoverContent
      aria-label={labels.more}
      className="inset-auto mt-2 mb-2 w-40 max-w-[calc(100vw-24px)] [justify-self:end] [position-anchor:--assistant-more] [position-area:top_span-left] [position-try-fallbacks:flip-block,flip-inline] motion-safe:animate-assistant-pop160"
    >
      {actions.map((action) => (
        <ActionPopoverItem
          key={action.icon}
          disabled={!action.run}
          className={cn(action.danger === true && "text-danger")}
          onClick={() => {
            close();
            action.run?.();
          }}
        >
          <Icon name={action.icon} className="size-4" />
          {action.label}
        </ActionPopoverItem>
      ))}
    </ActionPopoverContent>
  );
}

function Reactions({
  labels,
  reaction,
  onReact,
  quickReactions,
  reactionGroups,
}: {
  labels: MessageActionsLabels;
  reaction: string | null;
  onReact: MessageActionsProps["onReact"];
  quickReactions: readonly EmojiEntry[];
  reactionGroups: readonly EmojiGroup[];
}) {
  const { close } = useActionPopover();
  const [searching, setSearching] = useState(false);
  const [query, setQuery] = useState("");
  const [leavingExtra, setLeavingExtra] = useState<string | null>(null);
  const searchRef = useRef<HTMLInputElement>(null);
  const reset = () => {
    setSearching(false);
    setQuery("");
    setLeavingExtra(null);
  };
  const choose = (emoji: string) => {
    onReact(reaction === emoji ? null : emoji);
    if (!searching && reaction === emoji && !quickReactions.some(([quick]) => quick === emoji)) {
      if (!window.matchMedia("(prefers-reduced-motion: reduce)").matches) {
        setLeavingExtra(emoji);
      }
      return;
    }
    close();
    reset();
  };
  useEffect(() => {
    if (searching) {
      searchRef.current?.focus();
    }
  }, [searching]);
  const lowerQuery = query.trim().toLocaleLowerCase();
  const groups = reactionGroups
    .map((group) => ({
      ...group,
      items: group.items.filter(
        ([emoji, keywords]) =>
          !lowerQuery ||
          keywords.toLocaleLowerCase().includes(lowerQuery) ||
          emoji.includes(lowerQuery),
      ),
    }))
    .filter((group) => group.items.length > 0);
  const extra =
    reaction !== null && !quickReactions.some(([emoji]) => emoji === reaction)
      ? reaction
      : leavingExtra;
  return (
    <ActionPopoverContent
      aria-label={searching ? labels.search : labels.react}
      onToggle={(event) => {
        if (event.newState === "closed") {
          reset();
        }
      }}
      className={cn(
        "inset-auto mt-2 mb-2 [position-anchor:--assistant-reactions] [position-area:top_span-right] [position-try-fallbacks:flip-block,flip-inline] motion-safe:animate-assistant-pop160",
        searching ? "w-80 max-w-[calc(100vw-24px)]" : "w-max max-w-[calc(100vw-24px)]",
      )}
    >
      {searching ? (
        <>
          <div className="flex items-center gap-1 p-1">
            <Button
              variant="ghost"
              size="icon-sm"
              aria-label={labels.back}
              onClick={() => {
                setSearching(false);
                setQuery("");
              }}
            >
              <Icon name="back" className="size-4" />
            </Button>
            <label className="relative min-w-0 flex-1">
              <span className="sr-only">{labels.search}</span>
              <Input
                ref={searchRef}
                controlSize="compact"
                value={query}
                placeholder={labels.search}
                onChange={(event) => {
                  setQuery(event.target.value);
                }}
              />
            </label>
          </div>
          <div className="max-h-64 overflow-y-auto overscroll-contain p-1">
            {groups.length === 0 ? (
              <output className="block p-3 text-text-muted">{labels.empty}</output>
            ) : (
              groups.map((group) => (
                <section key={group.label} className="space-y-1">
                  <h3 className="px-2 pt-2 text-ui font-medium text-text-muted">{group.label}</h3>
                  <div className="grid grid-cols-8 gap-1 pointer-coarse:grid-cols-6">
                    {group.items.map(([emoji, keywords]) => (
                      <Button
                        key={emoji}
                        variant="ghost"
                        size="icon-sm"
                        selected={reaction === emoji}
                        aria-pressed={reaction === emoji}
                        aria-label={keywords}
                        onClick={() => {
                          choose(emoji);
                        }}
                      >
                        <span aria-hidden="true" className="text-body">
                          {emoji}
                        </span>
                      </Button>
                    ))}
                  </div>
                </section>
              ))
            )}
          </div>
        </>
      ) : (
        <div className="flex flex-wrap items-center gap-0.5">
          {quickReactions.map(([emoji, keywords]) => (
            <Button
              key={emoji}
              variant="ghost"
              size="icon"
              selected={reaction === emoji}
              aria-pressed={reaction === emoji}
              aria-label={keywords}
              onClick={() => {
                choose(emoji);
              }}
            >
              <span aria-hidden="true" className="text-body">
                {emoji}
              </span>
            </Button>
          ))}
          {extra === null ? null : (
            <Button
              variant="ghost"
              size="icon"
              selected={leavingExtra === null}
              aria-pressed={leavingExtra === null}
              aria-label={labels.reaction(extra)}
              className={cn(
                leavingExtra !== null &&
                  "pointer-events-none overflow-hidden motion-safe:animate-assistant-emoji-out220",
              )}
              onAnimationEnd={(event) => {
                if (event.target === event.currentTarget) {
                  setLeavingExtra(null);
                }
              }}
              onClick={() => {
                choose(extra);
              }}
            >
              <span aria-hidden="true" className="text-body">
                {extra}
              </span>
            </Button>
          )}
          <Button
            variant="ghost"
            size="icon"
            aria-label={labels.search}
            onClick={() => {
              setSearching(true);
            }}
          >
            <Icon name="plus" className="size-4" />
          </Button>
        </div>
      )}
    </ActionPopoverContent>
  );
}

function RegenerateAction({
  onRegenerate,
  regenerating = false,
  regenerateLabel = "Regenerate response",
  regeneratingLabel = "Regenerating response",
  disabled,
}: {
  onRegenerate: (() => void) | undefined;
  regenerating: boolean | undefined;
  regenerateLabel: string | undefined;
  regeneratingLabel: string | undefined;
  disabled: boolean;
}) {
  return (
    <>
      {onRegenerate ? (
        <ActionCue label={regenerating ? regeneratingLabel : regenerateLabel}>
          <Button
            variant="ghost"
            size="icon-sm"
            aria-label={regenerating ? regeneratingLabel : regenerateLabel}
            disabled={disabled || regenerating}
            onClick={onRegenerate}
          >
            <Icon
              name={regenerating ? "loading" : "history"}
              className={regenerating ? "motion-safe:animate-assistant-spin" : undefined}
            />
          </Button>
        </ActionCue>
      ) : null}
      <output className="sr-only">{regenerating ? regeneratingLabel : ""}</output>
    </>
  );
}

/** Voting, acknowledged copy, searchable reactions, reply, extra actions, and a quiet timestamp. */
export function MessageActions({
  label,
  labels,
  text,
  vote,
  onVote,
  reaction,
  onReact,
  copied,
  onCopy,
  onReply,
  onAloud,
  onFork,
  onReport,
  onRegenerate,
  regenerating,
  regenerateLabel,
  regeneratingLabel,
  disabled = false,
  timestamp,
  reactionGroups = MESSAGE_REACTION_GROUPS,
  quickReactions = QUICK_REACTIONS,
  className,
  ...props
}: MessageActionsProps) {
  const [upAnimation, setUpAnimation] = useState(0);
  const [downAnimation, setDownAnimation] = useState(0);
  const [smileAnimation, setSmileAnimation] = useState(0);
  const [replyAnimation, setReplyAnimation] = useState(0);
  const reactionLabel = reaction === null ? labels.react : labels.reaction(reaction);
  const statusId = useId();
  const cueId = useId();
  const [cueState, setCueState] = useState<CueState>(null);
  const cue = useMemo(
    () => ({
      current: cueState,
      id: cueId,
      show: (anchor: string, nextLabel: string) => {
        setCueState({ anchor, label: nextLabel });
      },
      hide: () => {
        setCueState(null);
      },
    }),
    [cueId, cueState],
  );
  return (
    <CueContext value={cue}>
      <CueBoundary>
        <fieldset
          {...props}
          disabled={disabled}
          aria-label={label}
          className={cn(
            "group/actions relative inline-flex flex-wrap items-center gap-0.5 [anchor-scope:all]",
            className,
          )}
        >
          <ActionCue label={labels.good}>
            <Button
              variant="ghost"
              size="icon-sm"
              selected={vote === "up"}
              disabled={disabled}
              aria-label={labels.good}
              aria-pressed={vote === "up"}
              onClick={() => {
                onVote(vote === "up" ? null : "up");
                setUpAnimation((count) => count + 1);
              }}
            >
              <Icon
                key={upAnimation}
                name="thumbs-up"
                className={cn(
                  "size-4",
                  vote === "up" && "text-success",
                  upAnimation > 0 && "motion-safe:animate-assistant-thumb-up",
                )}
              />
            </Button>
          </ActionCue>
          <ActionCue label={labels.bad}>
            <Button
              variant="ghost"
              size="icon-sm"
              selected={vote === "down"}
              disabled={disabled}
              aria-label={labels.bad}
              aria-pressed={vote === "down"}
              onClick={() => {
                onVote(vote === "down" ? null : "down");
                setDownAnimation((count) => count + 1);
              }}
            >
              <Icon
                key={downAnimation}
                name="thumbs-down"
                className={cn(
                  "size-4",
                  vote === "down" && "text-danger",
                  downAnimation > 0 && "motion-safe:animate-assistant-thumb-down",
                )}
              />
            </Button>
          </ActionCue>
          <RegenerateAction
            onRegenerate={onRegenerate}
            regenerating={regenerating}
            regenerateLabel={regenerateLabel}
            regeneratingLabel={regeneratingLabel}
            disabled={disabled}
          />
          <ActionCue label={copied ? labels.copied : labels.copy}>
            <Button
              variant="ghost"
              size="icon-sm"
              disabled={disabled}
              aria-label={copied ? labels.copied : labels.copy}
              aria-describedby={statusId}
              onClick={() => {
                onCopy(text);
              }}
            >
              <span className="grid">
                <Icon
                  name="copy"
                  className={cn(
                    "col-start-1 row-start-1 size-4 transition-opacity duration-220 motion-reduce:transition-none",
                    copied && "opacity-0 transition-none",
                  )}
                />
                <Icon
                  name="check"
                  strokeDasharray={28}
                  className={cn(
                    "col-start-1 row-start-1 size-4 text-success transition-opacity duration-220 motion-reduce:transition-none",
                    copied
                      ? "opacity-100 transition-none motion-safe:[&_path]:animate-assistant-check"
                      : "opacity-0",
                  )}
                />
              </span>
            </Button>
          </ActionCue>
          <output id={statusId} className="sr-only">
            {copied ? labels.copied : ""}
          </output>
          <ActionPopoverRoot>
            <ActionCue label={reactionLabel}>
              <ActionPopoverTrigger
                size="menu"
                disabled={disabled}
                aria-label={reactionLabel}
                className="size-control-compact justify-center p-0 [anchor-name:--assistant-reactions] pointer-coarse:size-control-touch"
                onClick={() => {
                  setSmileAnimation((count) => count + 1);
                }}
              >
                {reaction === null ? (
                  <span
                    key={smileAnimation}
                    className={cn(
                      "grid",
                      smileAnimation > 0 && "motion-safe:animate-assistant-smile",
                    )}
                  >
                    <Icon
                      name="smile"
                      className={cn(
                        "col-start-1 row-start-1 size-4",
                        smileAnimation > 0 && "motion-safe:animate-assistant-wink-shut",
                      )}
                    />
                    <Icon
                      name="smile-wink"
                      className={cn(
                        "col-start-1 row-start-1 size-4 opacity-0",
                        smileAnimation > 0 && "motion-safe:animate-assistant-wink-line",
                      )}
                    />
                  </span>
                ) : (
                  <span
                    key={reaction}
                    aria-hidden="true"
                    className="text-body motion-safe:animate-assistant-reaction"
                  >
                    {reaction}
                  </span>
                )}
              </ActionPopoverTrigger>
            </ActionCue>
            <Reactions
              labels={labels}
              reaction={reaction}
              onReact={onReact}
              quickReactions={quickReactions}
              reactionGroups={reactionGroups}
            />
          </ActionPopoverRoot>
          <ActionCue label={labels.reply}>
            <Button
              variant="ghost"
              size="icon-sm"
              disabled={disabled}
              aria-label={labels.reply}
              onClick={() => {
                onReply();
                setReplyAnimation((count) => count + 1);
              }}
            >
              <Icon
                key={replyAnimation}
                name="reply"
                className={cn(
                  "size-4 [&_path]:[transform-box:view-box] [&_path:nth-of-type(2)]:origin-[66.667%_50%]",
                  replyAnimation > 0 &&
                    "motion-safe:[&_path:nth-of-type(1)]:animate-assistant-reply-head520 motion-safe:[&_path:nth-of-type(2)]:animate-assistant-reply",
                )}
              />
            </Button>
          </ActionCue>
          <ActionPopoverRoot>
            <ActionCue label={labels.more}>
              <ActionPopoverTrigger
                size="menu"
                disabled={disabled}
                aria-label={labels.more}
                className="size-control-compact justify-center p-0 [anchor-name:--assistant-more] pointer-coarse:size-control-touch"
              >
                <Icon name="dots" className="size-4" />
              </ActionPopoverTrigger>
            </ActionCue>
            <MoreActions labels={labels} onAloud={onAloud} onFork={onFork} onReport={onReport} />
          </ActionPopoverRoot>
          {timestamp ? (
            <ActionCue label={timestamp.absolute}>
              <time
                dateTime={timestamp.dateTime}
                className="ml-1 inline-flex h-5 items-center text-ui whitespace-nowrap text-text-muted opacity-0 transition-opacity duration-200 group-focus-within/actions:opacity-100 group-hover/actions:opacity-100 motion-reduce:transition-none pointer-coarse:opacity-100"
              >
                <AnimatedTime value={timestamp.relative} />
              </time>
            </ActionCue>
          ) : null}
          <RowCue id={cueId} current={cueState} />
        </fieldset>
      </CueBoundary>
    </CueContext>
  );
}
