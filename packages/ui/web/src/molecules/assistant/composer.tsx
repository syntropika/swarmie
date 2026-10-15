import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
} from "react";
import type { ComponentPropsWithRef, ReactNode } from "react";

import { Avatar } from "../../atoms/avatar";
import { Button } from "../../atoms/button";
import { Icon } from "../../atoms/icon";
import { Input } from "../../atoms/input";
import {
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectLabel,
  SelectRoot,
  SelectTrigger,
  SelectValue,
} from "../../atoms/select";
import { Textarea } from "../../atoms/textarea";
import { TooltipContent, TooltipProvider, TooltipRoot, TooltipTrigger } from "../../atoms/tooltip";
import { cn } from "../../classes";
import { FileAttachment } from "./attachment";
import type { AttachmentImage, AttachmentStatus } from "./attachment";
import { nextComposerItem } from "./composer-matching";
import { ConversationArrival } from "./conversation-motion";
import { ReasoningEffort } from "./reasoning-effort";
import {
  AssistantDisclosure,
  AssistantPanel,
  AssistantProgress,
  Facts,
  PanelHeading,
  TooltipIconButton,
} from "./shared";

type ComposerContextValue = {
  value: string;
  onValueChange: (value: string) => void;
  busy: boolean;
  disabled: boolean;
  canSubmit: boolean;
  submit: () => void;
  inputId: string;
};
const ComposerContext = createContext<ComposerContextValue | undefined>(undefined);

function useComposer() {
  const context = useContext(ComposerContext);
  if (!context) {
    throw new Error("Composer parts require ComposerRoot.");
  }
  return context;
}

export type ComposerRootProps = Omit<ComponentPropsWithRef<"form">, "onSubmit"> & {
  value: string;
  onValueChange: (value: string) => void;
  onSubmit: (value: string) => void;
  busy?: boolean;
  disabled?: boolean;
  allowEmpty?: boolean;
  onFiles?: (files: readonly File[]) => void;
  dropLabel?: string;
};

/** Controlled input contract. The host performs transport, draft persistence, and cancellation. */
export function ComposerRoot({
  value,
  onValueChange,
  onSubmit,
  busy = false,
  disabled = false,
  allowEmpty = false,
  onFiles,
  dropLabel,
  onDragOver,
  onDrop,
  className,
  children,
  ...props
}: ComposerRootProps) {
  const inputId = useId();
  const canSubmit = !disabled && !busy && (allowEmpty || value.trim().length > 0);
  const submit = useCallback(() => {
    if (canSubmit) {
      onSubmit(value.trim());
    }
  }, [canSubmit, value, onSubmit]);
  const context = useMemo(
    () => ({ value, onValueChange, busy, disabled, canSubmit, submit, inputId }),
    [value, onValueChange, busy, disabled, canSubmit, inputId, submit],
  );
  return (
    <ComposerContext value={context}>
      {/* oxlint-disable-next-line jsx-a11y/no-noninteractive-element-interactions -- File drop augments the keyboard-accessible attachment input. */}
      <form
        {...props}
        onSubmit={(event) => {
          event.preventDefault();
          submit();
        }}
        onDragOver={(event) => {
          onDragOver?.(event);
          if (!disabled && onFiles && event.dataTransfer.types.includes("Files")) {
            event.preventDefault();
          }
        }}
        onDrop={(event) => {
          onDrop?.(event);
          if (!disabled && onFiles && event.dataTransfer.files.length) {
            event.preventDefault();
            onFiles([...event.dataTransfer.files]);
          }
        }}
        aria-busy={busy}
        className={cn(
          "@container flex min-w-0 flex-col gap-2 rounded-panel border border-transparent bg-surface-muted p-3 transition-shadow duration-150 ease-swarmie has-focus-visible:shadow-overlay motion-reduce:transition-none",
          className,
        )}
      >
        {dropLabel === undefined ? null : <span className="sr-only">{dropLabel}</span>}
        <fieldset disabled={disabled} className="contents">
          {children}
        </fieldset>
      </form>
    </ComposerContext>
  );
}

export function ComposerInput({
  label,
  className,
  ...props
}: Omit<ComponentPropsWithRef<"textarea">, "value" | "onChange" | "disabled" | "id"> & {
  label: string;
}) {
  const context = useComposer();
  return (
    <label className="block">
      <span className="sr-only">{label}</span>
      <Textarea
        {...props}
        appearance="bare"
        id={context.inputId}
        disabled={context.disabled}
        value={context.value}
        onChange={(event) => {
          context.onValueChange(event.target.value);
        }}
        onKeyDown={(event) => {
          props.onKeyDown?.(event);
          if (
            event.key === "Enter" &&
            !event.shiftKey &&
            !event.nativeEvent.isComposing &&
            !event.defaultPrevented
          ) {
            event.preventDefault();
            context.submit();
          }
        }}
        className={cn(
          "[field-sizing:content] max-h-48 min-h-control-entry resize-none border-0 p-0 text-body leading-body font-normal focus-visible:outline-offset-4",
          className,
        )}
      />
    </label>
  );
}

export function ComposerRail({ className, ...props }: ComponentPropsWithRef<"div">) {
  return (
    <div {...props} className={cn("flex flex-wrap items-center justify-end gap-2", className)} />
  );
}

export function ComposerSubmit({
  label,
  stopLabel,
  onStop,
}: {
  label: string;
  stopLabel: string;
  onStop: () => void;
}) {
  const context = useComposer();
  const accessibleLabel = context.busy ? stopLabel : label;
  return (
    <Button
      type={context.busy ? "button" : "submit"}
      variant={context.busy ? "secondary" : "primary"}
      size="icon"
      aria-label={accessibleLabel}
      title={accessibleLabel}
      disabled={context.disabled || (!context.busy && !context.canSubmit)}
      onClick={() => {
        if (context.busy) {
          onStop();
        }
      }}
    >
      <span className="grid">
        <Icon
          name="arrow"
          className={cn(
            "col-start-1 row-start-1 -rotate-90 transition-all duration-200 motion-reduce:transition-none",
            context.busy && "scale-50 opacity-0",
          )}
        />
        <Icon
          name="stop"
          className={cn(
            "col-start-1 row-start-1 transition-all duration-200 motion-reduce:transition-none",
            !context.busy && "scale-50 opacity-0",
          )}
        />
      </span>
    </Button>
  );
}

export type ComposerAttachment = {
  id: string;
  name: string;
  detail: string;
  progress?: number;
  status?: AttachmentStatus;
  image?: AttachmentImage;
};

export function ComposerAttachments({
  files,
  removeLabel,
  onRemove,
  previewLabel,
  closeLabel,
}: {
  files: readonly ComposerAttachment[];
  removeLabel: string;
  previewLabel?: (name: string) => string;
  closeLabel?: string;
  onRemove: (id: string) => void;
}) {
  if (files.length === 0) {
    return null;
  }
  return (
    <ul className="flex flex-wrap items-start gap-2">
      {files.map((file) => (
        <li key={file.id}>
          <FileAttachment
            name={file.name}
            detail={file.detail}
            progress={file.progress}
            status={file.status}
            image={file.image}
            previewLabel={previewLabel?.(file.name)}
            closeLabel={closeLabel}
            removeLabel={removeLabel}
            onRemove={() => {
              onRemove(file.id);
            }}
          />
        </li>
      ))}
    </ul>
  );
}

export function ComposerAttachButton({
  label,
  accept,
  multiple = true,
  onFiles,
  disabled = false,
}: {
  disabled?: boolean;
  label: string;
  accept?: string;
  multiple?: boolean;
  onFiles: (files: readonly File[]) => void;
}) {
  const id = useId();
  return (
    <span className="mr-auto">
      <input
        id={id}
        type="file"
        disabled={disabled}
        className="peer sr-only"
        accept={accept}
        multiple={multiple}
        onChange={(event) => {
          onFiles([...(event.target.files ?? [])]);
          event.target.value = "";
        }}
      />
      <label
        htmlFor={id}
        className="inline-flex size-control cursor-pointer items-center justify-center rounded-pill p-0 text-ui leading-ui font-medium peer-focus-visible:outline-2 peer-focus-visible:outline-primary hover:bg-surface-hover pointer-coarse:size-control-touch"
      >
        <Icon name="plus" />
        <span className="sr-only">{label}</span>
      </label>
    </span>
  );
}

export type ModelOption = {
  id: string;
  name: string;
  provider: string;
  detail: string;
  disabled?: boolean;
  efforts?: readonly { value: string; label: string; budget: number }[];
};

export function ModelSelector({
  label,
  models,
  value,
  onChange,
  disabled = false,
  searchLabel,
  emptyLabel = "No matching models",
  effort,
  onEffortChange,
  effortLabel = "Reasoning effort",
}: {
  disabled?: boolean;
  searchLabel?: string;
  emptyLabel?: string;
  effort?: string;
  onEffortChange?: (value: string) => void;
  effortLabel?: string;
  label: string;
  models: readonly ModelOption[];
  value: string;
  onChange: (id: string) => void;
}) {
  const [query, setQuery] = useState("");
  const visible = models.filter((model) =>
    `${model.name} ${model.provider} ${model.detail}`
      .toLocaleLowerCase()
      .includes(query.toLocaleLowerCase()),
  );
  const selected = models.find((model) => model.id === value);
  const providers = [...new Set(visible.map((model) => model.provider))];
  return (
    <div className="space-y-2">
      {searchLabel === undefined ? null : (
        <label className="flex flex-col gap-1 text-ui">
          {searchLabel}
          <Input
            type="search"
            value={query}
            disabled={disabled}
            onChange={(event) => {
              setQuery(event.target.value);
            }}
          />
        </label>
      )}
      <SelectRoot value={value} onValueChange={onChange} disabled={disabled}>
        <SelectTrigger aria-label={label} variant="ghost" className="max-w-full">
          <Icon name="agent" className="size-3.5 text-text-muted" />
          <span className="min-w-0 truncate">
            <SelectValue>{models.find((model) => model.id === value)?.name}</SelectValue>
          </span>
        </SelectTrigger>
        <SelectContent
          align="start"
          side="top"
          className="assistant-model-menu! w-80 motion-reduce:animate-none!"
        >
          {visible.length ? null : <p className="p-3 text-ui text-text-muted">{emptyLabel}</p>}
          {providers.map((provider) => (
            <SelectGroup key={provider}>
              <SelectLabel>{provider}</SelectLabel>
              {visible
                .filter((model) => model.provider === provider)
                .map((model) => (
                  <SelectItem
                    key={model.id}
                    value={model.id}
                    disabled={model.disabled ?? false}
                    description={model.detail}
                    textValue={model.name}
                  >
                    {model.name}
                  </SelectItem>
                ))}
            </SelectGroup>
          ))}
        </SelectContent>
      </SelectRoot>
      {selected?.efforts && effort !== undefined ? (
        <ReasoningEffort
          spent={0}
          usageLabel={effortLabel}
          label={effortLabel}
          value={effort}
          options={selected.efforts}
          {...(!disabled && onEffortChange ? { onChange: onEffortChange } : {})}
        />
      ) : null}
    </div>
  );
}

export type PickerItem = {
  id: string;
  label: string;
  description?: string;
  group?: string;
  initials?: string;
  keys?: readonly string[];
  children?: readonly PickerItem[];
  insertion?: string;
  behavior?: "directive" | "action";
  disabled?: boolean;
};

/** Search keeps focus in the input while the active result follows grouped display order. */
export function CommandPalette({
  label,
  placeholder,
  items,
  onSelect,
  emptyLabel,
}: {
  label: string;
  placeholder: string;
  items: readonly PickerItem[];
  onSelect: (id: string) => void;
  emptyLabel: string;
}) {
  const [query, setQuery] = useState("");
  const [activeId, setActiveId] = useState<string>();
  const listId = useId();
  const listRef = useRef<HTMLDivElement>(null);
  const visible = items.filter((item) =>
    `${item.label} ${item.description ?? ""}`
      .toLocaleLowerCase()
      .includes(query.toLocaleLowerCase()),
  );
  const groups = [...new Set(visible.map((item) => item.group ?? ""))];
  const ordered = groups.flatMap((group) => visible.filter((item) => (item.group ?? "") === group));
  const active = ordered.find((item) => item.id === activeId && item.disabled !== true);
  useEffect(() => {
    listRef.current
      ?.querySelector(`[data-command-id="${CSS.escape(activeId ?? "")}"]`)
      ?.scrollIntoView({ block: "nearest" });
  }, [activeId]);
  return (
    <AssistantPanel>
      <label className="flex flex-col gap-2 text-ui font-medium">
        {label}
        <Input
          // oxlint-disable-next-line jsx-a11y/prefer-tag-over-role -- Input renders a native input with searchable composite semantics.
          role="combobox"
          aria-autocomplete="list"
          aria-expanded={ordered.length > 0}
          aria-controls={listId}
          aria-activedescendant={active ? `${listId}-${active.id}` : undefined}
          value={query}
          placeholder={placeholder}
          onChange={(event) => {
            setQuery(event.target.value);
          }}
          onKeyDown={(event) => {
            if (event.nativeEvent.isComposing) {
              return;
            }
            if (event.key === "ArrowDown" || event.key === "ArrowUp") {
              event.preventDefault();
              setActiveId(
                nextComposerItem(ordered, activeId, event.key === "ArrowDown" ? 1 : -1)?.id,
              );
            }
            if (event.key === "Enter" && active) {
              event.preventDefault();
              onSelect(active.id);
            }
          }}
        />
      </label>
      <div
        ref={listRef}
        id={listId}
        // oxlint-disable-next-line jsx-a11y/prefer-tag-over-role -- Rich command options require the composite listbox contract.
        role="listbox"
        aria-label={label}
        className="mt-3 max-h-64 space-y-2 overflow-y-auto"
      >
        {groups.map((group) => (
          <fieldset key={group} aria-label={group || label}>
            {group ? <p className="px-3 text-xs text-text-muted">{group}</p> : null}
            {ordered
              .filter((item) => (item.group ?? "") === group)
              .map((item) => (
                <Button
                  data-command-id={item.id}
                  key={item.id}
                  id={`${listId}-${item.id}`}
                  // oxlint-disable-next-line jsx-a11y/prefer-tag-over-role -- This rich prompt option participates in the input-owned listbox.
                  role="option"
                  aria-selected={activeId === item.id}
                  selected={activeId === item.id}
                  className="w-full"
                  variant="navigation"
                  disabled={item.disabled}
                  onClick={() => {
                    setActiveId(item.id);
                    onSelect(item.id);
                  }}
                >
                  <span className="min-w-0 flex-1">
                    <span className="block font-medium">{item.label}</span>
                    {item.description === undefined ? null : (
                      <span className="block text-xs text-text-muted">{item.description}</span>
                    )}
                  </span>
                  {item.keys?.map((key) => (
                    <kbd key={key} className="text-xs text-text-muted">
                      {key}
                    </kbd>
                  ))}
                </Button>
              ))}
          </fieldset>
        ))}
      </div>
      {ordered.length === 0 ? <p className="py-3 text-ui text-text-muted">{emptyLabel}</p> : null}
    </AssistantPanel>
  );
}

export function ComposerTriggerPicker({
  trigger,
  query,
  items,
  label,
  onSelect,
  open = true,
  backLabel = "Back",
  onInsert,
  onAction,
}: {
  trigger: string;
  query: string;
  items: readonly PickerItem[];
  label: string;
  onSelect: (id: string) => void;
  open?: boolean;
  backLabel?: string;
  onInsert?: (text: string) => void;
  onAction?: (id: string) => void;
}) {
  const [path, setPath] = useState<readonly PickerItem[]>([]);
  const [activeId, setActiveId] = useState<string>();
  const parent = path.at(-1);
  const visible = (parent?.children ?? items).filter((item) =>
    item.label.toLocaleLowerCase().startsWith(query.toLocaleLowerCase()),
  );
  const refs = useRef(new Map<string, HTMLButtonElement>());
  const pick = (item: PickerItem) => {
    if (item.children) {
      setPath((current) => [...current, item]);
      setActiveId(undefined);
      return;
    }
    if (item.behavior === "action") {
      onAction?.(item.id);
    } else if (item.insertion !== undefined) {
      onInsert?.(item.insertion);
    }
    onSelect(item.id);
  };
  if (!open || visible.length === 0) {
    return null;
  }
  return (
    <ConversationArrival duration={200} motion="menu">
      <section aria-label={label} className="min-w-0 rounded-panel bg-surface p-1">
        {parent ? (
          <Button
            variant="ghost"
            onClick={() => {
              setPath((current) => current.slice(0, -1));
            }}
          >
            {backLabel}
          </Button>
        ) : null}
        <ul className="space-y-1">
          {visible.map((item) => (
            <li key={item.id}>
              <Button
                ref={(element) => {
                  if (element) {
                    refs.current.set(item.id, element);
                  } else {
                    refs.current.delete(item.id);
                  }
                }}
                className="w-full"
                variant="navigation"
                selected={item.id === activeId}
                disabled={item.disabled}
                onKeyDown={(event) => {
                  if (event.key === "ArrowUp" || event.key === "ArrowDown") {
                    event.preventDefault();
                    const next = nextComposerItem(
                      visible,
                      item.id,
                      event.key === "ArrowDown" ? 1 : -1,
                    );
                    if (next) {
                      setActiveId(next.id);
                      refs.current.get(next.id)?.focus();
                    }
                  }
                  if (event.key === "Escape" && path.length > 0) {
                    event.preventDefault();
                    setPath((current) => current.slice(0, -1));
                  }
                }}
                onClick={() => {
                  pick(item);
                }}
              >
                {item.initials === undefined ? (
                  <span className="font-mono text-code text-primary">{trigger}</span>
                ) : (
                  <Avatar kind="person" label={item.label} initials={item.initials} />
                )}
                <span className="min-w-0">
                  <span className="block font-medium">{item.label}</span>
                  <span className="block text-xs text-text-muted">{item.description}</span>
                </span>
                {item.children ? <Icon name="arrow" /> : null}
              </Button>
            </li>
          ))}
        </ul>
      </section>
    </ConversationArrival>
  );
}

/** Composer popup detail must be descriptive; interactive controls belong in the disclosure preset. */
export function ContextDisplay({
  label,
  used,
  maximum,
  detail,
  preset = "ring",
  segments,
  presentation = "disclosure",
}: {
  preset?: "ring" | "bar" | "text";
  presentation?: "disclosure" | "popup";
  segments?: readonly ContextSegment[];
  label: string;
  used: number;
  maximum: number;
  detail?: ReactNode;
}) {
  const ratio = Math.min(1, Math.max(0, used / Math.max(1, maximum)));
  const composer = presentation === "popup";
  let severity = "stroke-primary";
  if (ratio >= 0.85) {
    severity = "stroke-danger";
  } else if (ratio >= 0.65) {
    severity = "stroke-attention";
  }
  const geometry = (
    <ContextUsageGraphic preset={preset} ratio={ratio} severity={severity} composer={composer} />
  );
  const content = (
    <div className="space-y-3">
      <p className="flex justify-between gap-3 text-ui font-medium">
        <span>{label}</span>
        <span className="tabular-nums">{Math.round(ratio * 100)}%</span>
      </p>
      <AssistantProgress label={label} value={used} max={Math.max(1, maximum)} />
      {segments ? (
        <ContextBreakdown
          label={label}
          maximum={maximum}
          segments={segments}
          motion={composer ? "composer" : "default"}
        />
      ) : null}
      {detail}
    </div>
  );
  if (composer) {
    return (
      <TooltipProvider>
        <TooltipRoot>
          <TooltipTrigger asChild>
            <Button
              variant="ghost"
              size="icon"
              aria-label={`${label}: ${Math.round(ratio * 100)}%`}
            >
              {geometry}
            </Button>
          </TooltipTrigger>
          <TooltipContent
            side="top"
            align="end"
            className="w-72 rounded-panel bg-surface p-3 text-ink motion-safe:animate-assistant-context-open! data-[state=closed]:motion-safe:animate-assistant-context-close! motion-reduce:animate-none!"
          >
            {content}
          </TooltipContent>
        </TooltipRoot>
      </TooltipProvider>
    );
  }
  return (
    <AssistantDisclosure
      summary={
        <span className="flex items-center gap-3">
          {geometry}
          <span>{label}</span>
          <span className="ml-auto text-xs tabular-nums">{Math.round(ratio * 100)}%</span>
        </span>
      }
    >
      {content}
    </AssistantDisclosure>
  );
}

function ContextUsageGraphic({
  preset,
  ratio,
  severity,
  composer,
}: {
  preset: "ring" | "bar" | "text";
  ratio: number;
  severity: string;
  composer: boolean;
}) {
  const transition = composer ? "duration-700" : "duration-300";
  if (preset === "bar") {
    return (
      <svg
        viewBox="0 0 100 4"
        aria-hidden="true"
        className="h-2 w-20 overflow-hidden rounded-full bg-surface-muted"
      >
        <rect
          width={ratio * 100}
          height="4"
          className={cn("fill-primary transition-all motion-reduce:transition-none", transition)}
        />
      </svg>
    );
  }
  if (preset === "text") {
    return composer ? (
      <span className="text-xs tabular-nums">{Math.round(ratio * 100)}%</span>
    ) : null;
  }
  return (
    <svg
      viewBox="0 0 40 40"
      aria-hidden="true"
      className={cn("-rotate-90", composer ? "size-4" : "size-7")}
    >
      <circle cx="20" cy="20" r="16" fill="none" strokeWidth="3" className="stroke-border" />
      <circle
        cx="20"
        cy="20"
        r="16"
        fill="none"
        strokeWidth="3"
        strokeLinecap="round"
        strokeDasharray={100.53}
        strokeDashoffset={100.53 * (1 - ratio)}
        className={cn("transition-all motion-reduce:transition-none", transition, severity)}
      />
    </svg>
  );
}

export type ContextSegment = { id: string; label: string; tokens: number; detail: string };

export function ContextBreakdown({
  label,
  segments,
  maximum,
  remainingLabel = "Remaining context",
  motion = "default",
}: {
  remainingLabel?: string;
  motion?: "default" | "composer";
  label: string;
  segments: readonly ContextSegment[];
  maximum: number;
}) {
  const used = segments.reduce((total, segment) => total + Math.max(0, segment.tokens), 0);
  const colors = ["fill-primary", "fill-success", "fill-attention", "fill-text-muted"];
  return (
    <AssistantPanel>
      <PanelHeading>{label}</PanelHeading>
      <p className="mb-2 text-xs text-text-muted tabular-nums">
        {used} / {maximum} · {remainingLabel}: {Math.max(0, maximum - used)}
      </p>
      <svg
        viewBox="0 0 100 4"
        aria-label={label}
        className="mb-3 h-2 w-full overflow-hidden rounded-full bg-surface-muted"
      >
        {segments.map((segment, index) => {
          const before = segments
            .slice(0, index)
            .reduce((total, item) => total + Math.max(0, item.tokens), 0);
          return (
            <rect
              key={segment.id}
              x={(before / Math.max(1, maximum)) * 100}
              y="0"
              width={(Math.max(0, segment.tokens) / Math.max(1, maximum)) * 100}
              height="4"
              className={cn(
                "transition-all motion-reduce:transition-none",
                motion === "composer" ? "duration-700" : "duration-500 ease-out",
                colors[index % colors.length],
              )}
            >
              <title>
                {segment.label}: {segment.tokens}
              </title>
            </rect>
          );
        })}
      </svg>
      <ul className="space-y-3">
        {segments.map((segment) => (
          <li key={segment.id}>
            <div className="flex justify-between gap-3 text-ui">
              <span>{segment.label}</span>
              <span className="tabular-nums">{segment.tokens}</span>
            </div>
            <AssistantProgress
              label={segment.label}
              value={segment.tokens}
              max={Math.max(1, maximum)}
            />
            <p className="text-xs text-text-muted">{segment.detail}</p>
          </li>
        ))}
      </ul>
    </AssistantPanel>
  );
}

export function Dictation({
  label,
  transcript,
  active,
  onToggle,
  stopLabel,
  tick = 0,
}: {
  tick?: number;
  label: string;
  transcript: string;
  active: boolean;
  onToggle: () => void;
  stopLabel: string;
}) {
  return (
    <div className="flex flex-wrap items-center gap-3">
      <TooltipIconButton
        label={active ? stopLabel : label}
        icon={active ? "stop" : "microphone"}
        aria-pressed={active}
        onClick={onToggle}
      />
      <output className="min-w-0 flex-1 text-body leading-body" aria-live="polite">
        {transcript}
      </output>
      {active ? (
        <svg aria-hidden="true" viewBox="0 0 100 32" className="h-8 w-24 text-primary">
          {Array.from({ length: 14 }, (_, bar) => {
            const height = 5 + Math.abs(Math.sin(bar * 1.35 + tick * 0.55)) * 13;
            return (
              <rect
                key={bar}
                x={bar * 7}
                y={(32 - height) / 2}
                width="2"
                height={height}
                rx="1"
                className="fill-current transition-all duration-150 motion-reduce:transition-none"
              />
            );
          })}
        </svg>
      ) : null}
    </div>
  );
}

export function DraftRestore({
  label,
  preview,
  restoreLabel,
  discardLabel,
  onRestore,
  onDiscard,
  savedAt,
}: {
  savedAt?: string;
  label: string;
  preview: string;
  restoreLabel: string;
  discardLabel: string;
  onRestore: () => void;
  onDiscard: () => void;
}) {
  return (
    <AssistantPanel className="motion-safe:animate-assistant-receipt">
      <PanelHeading>{label}</PanelHeading>
      {savedAt === undefined ? null : <p className="text-xs text-text-muted">{savedAt}</p>}
      <p className="mb-3 text-ui text-text-muted">{preview}</p>
      <div className="flex flex-wrap gap-2">
        <Button variant="secondary" onClick={onRestore}>
          {restoreLabel}
        </Button>
        <Button variant="ghost" onClick={onDiscard}>
          {discardLabel}
        </Button>
      </div>
    </AssistantPanel>
  );
}

export type SavedPrompt = {
  id: string;
  title: string;
  text: string;
  variables: readonly { name: string; label: string }[];
};

export function PromptLibrary({
  label,
  prompts,
  searchLabel,
  insertLabel,
  onInsert,
}: {
  label: string;
  prompts: readonly SavedPrompt[];
  searchLabel: string;
  insertLabel: string;
  onInsert: (id: string, values: Readonly<Record<string, string>>) => void;
}) {
  const [query, setQuery] = useState("");
  const [selectedId, setSelectedId] = useState<string>();
  const [values, setValues] = useState<Record<string, string>>({});
  const visible = prompts.filter((prompt) =>
    `${prompt.title} ${prompt.text}`.toLocaleLowerCase().includes(query.toLocaleLowerCase()),
  );
  const listId = useId();
  const selected = visible.find((prompt) => prompt.id === selectedId);
  return (
    <AssistantPanel>
      <PanelHeading>{label}</PanelHeading>
      <label className="flex flex-col gap-2 text-ui">
        {searchLabel}
        <Input
          // oxlint-disable-next-line jsx-a11y/prefer-tag-over-role -- Input renders a native input with searchable composite semantics.
          role="combobox"
          aria-expanded={visible.length > 0}
          aria-controls={listId}
          aria-autocomplete="list"
          aria-activedescendant={
            visible.some((prompt) => prompt.id === selectedId)
              ? `${listId}-${selectedId}`
              : undefined
          }
          onKeyDown={(event) => {
            if (event.nativeEvent.isComposing) {
              return;
            }
            if (event.key === "ArrowDown" || event.key === "ArrowUp") {
              event.preventDefault();
              setSelectedId(
                nextComposerItem(visible, selectedId, event.key === "ArrowDown" ? 1 : -1)?.id,
              );
              setValues({});
            }
            if (
              event.key === "Enter" &&
              selected &&
              selected.variables.every(
                (variable) => (values[variable.name]?.trim().length ?? 0) > 0,
              )
            ) {
              event.preventDefault();
              onInsert(selected.id, values);
            }
          }}
          value={query}
          onChange={(event) => {
            setQuery(event.target.value);
          }}
        />
      </label>
      {/* oxlint-disable-next-line jsx-a11y/prefer-tag-over-role, jsx-a11y/no-noninteractive-element-to-interactive-role -- Rich prompt options belong to the input-owned listbox. */}
      <ul id={listId} role="listbox" aria-label={label} className="my-3 space-y-1">
        {visible.map((prompt) => (
          <li key={prompt.id} role="presentation">
            <Button
              id={`${listId}-${prompt.id}`}
              // oxlint-disable-next-line jsx-a11y/prefer-tag-over-role -- This rich prompt option participates in the input-owned listbox.
              role="option"
              aria-selected={prompt.id === selectedId}
              onDoubleClick={() => {
                if (prompt.variables.length === 0) {
                  onInsert(prompt.id, {});
                }
              }}
              variant="navigation"
              className="w-full"
              selected={prompt.id === selectedId}
              onClick={() => {
                setSelectedId(prompt.id);
                setValues({});
              }}
            >
              {prompt.title}
            </Button>
          </li>
        ))}
      </ul>
      {selected ? (
        <ConversationArrival key={selected.id} duration={200} className="space-y-3">
          <p className="text-ui text-text-muted">{selected.text}</p>
          {selected.variables.map((variable) => (
            <label key={variable.name} className="flex flex-col gap-2 text-ui">
              {variable.label}
              <Input
                value={values[variable.name] ?? ""}
                onChange={(event) => {
                  setValues((current) => ({ ...current, [variable.name]: event.target.value }));
                }}
              />
            </label>
          ))}
          <Button
            disabled={selected.variables.some(
              (variable) => (values[variable.name]?.trim().length ?? 0) === 0,
            )}
            onClick={() => {
              onInsert(selected.id, values);
            }}
          >
            {insertLabel}
          </Button>
        </ConversationArrival>
      ) : null}
    </AssistantPanel>
  );
}

export function ModelDetails({ model }: { model: ModelOption }) {
  return (
    <Facts
      items={[
        { label: model.provider, value: model.name },
        { label: model.name, value: model.detail },
      ]}
    />
  );
}

/** Match only a trailing mention, preserving the text preceding it. */
export function matchComposerMention(value: string): string | null {
  return /@(?<query>[\w]*)$/u.exec(value)?.groups?.query ?? null;
}

export function applyComposerMention(value: string, name: string): string {
  return value.replace(/@[\w]*$/u, `@${name} `);
}

export function RegenerateMenu({
  label,
  currentLabel,
  models,
  currentId,
  open,
  onOpenChange,
  onPick,
}: {
  label: string;
  currentLabel: string;
  models: readonly ModelOption[];
  currentId: string;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onPick: (id: string) => void;
}) {
  const id = useId();
  return (
    <div className="space-y-2">
      <Button
        variant="ghost"
        aria-expanded={open}
        aria-controls={id}
        onClick={() => {
          onOpenChange(!open);
        }}
      >
        <Icon name="history" />
        {label}
      </Button>
      {open ? (
        <ConversationArrival duration={200} motion="menu">
          <div id={id} className="rounded-panel bg-surface p-1">
            {models.map((model) => (
              <Button
                key={model.id}
                className="w-full"
                variant="navigation"
                disabled={model.disabled}
                onClick={() => {
                  onPick(model.id);
                  onOpenChange(false);
                }}
              >
                <span className="flex-1 text-left">{model.name}</span>
                <span className="text-xs text-text-muted">
                  {model.id === currentId ? currentLabel : model.detail}
                </span>
              </Button>
            ))}
          </div>
        </ConversationArrival>
      ) : null}
    </div>
  );
}
