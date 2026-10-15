import { renderToString } from "katex";
import Prism from "prismjs";
import "prismjs/components/prism-jsx";
import "prismjs/components/prism-typescript";
import "prismjs/components/prism-tsx";
import "prismjs/components/prism-python";
import {
  createContext,
  useContext,
  isValidElement,
  useDeferredValue,
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
} from "react";
import type { ReactNode } from "react";
import ReactMarkdown from "react-markdown";
import type { Components } from "react-markdown";
import remarkGfm from "remark-gfm";
import type { BundledLanguage, SpecialLanguage } from "shiki";

import { Button } from "../../atoms/button";
import { Icon } from "../../atoms/icon";
import { SelectionControl } from "../../atoms/selection-control";
import { cn } from "../../classes";
import { DiagramViewport } from "./diagram-viewport";
import { CodeSurface } from "./shared";
import { AssistantModal } from "./thread";

const tokenClasses = new Map<string, string>(
  Object.entries({
    keyword: "text-inverse-link",
    string: "text-mint",
    number: "text-butter",
    comment: "text-inverse-muted",
    function: "text-lilac",
    punctuation: "text-inverse-text",
    operator: "text-inverse-link",
    boolean: "text-butter",
    "class-name": "text-peach",
  }),
);

const surfaceTokenClasses = new Map<string, string>(
  Object.entries({
    keyword: "text-danger",
    string: "text-primary",
    number: "text-primary",
    comment: "text-text-muted",
    function: "text-primary",
    boolean: "text-primary",
  }),
);

function prismContent(
  tokens: string | Prism.Token | (string | Prism.Token)[],
  prefix: string,
  tone: "inverse" | "surface" = "inverse",
): ReactNode {
  if (typeof tokens === "string") {
    return tokens;
  }
  if (Array.isArray(tokens)) {
    let offset = 0;
    return tokens.map((token) => {
      const key = `${prefix}-${offset}`;
      offset += token.length;
      return <span key={key}>{prismContent(token, key, tone)}</span>;
    });
  }
  return (
    <span
      className={
        tone === "surface" ? surfaceTokenClasses.get(tokens.type) : tokenClasses.get(tokens.type)
      }
    >
      {prismContent(tokens.content, prefix, tone)}
    </span>
  );
}

/** Reusable escaped token content for source panels and unified diffs. */
export function SyntaxTokens({
  source,
  language,
  tone = "inverse",
}: {
  source: string;
  language: string;
  tone?: "inverse" | "surface";
}): ReactNode {
  const grammar = Prism.languages[language];
  return prismContent(grammar ? Prism.tokenize(source, grammar) : source, language, tone);
}

/** Prism grammars produce React text nodes; source code is never executed or inserted as HTML. */
export function SyntaxHighlighter({
  source,
  language,
  label,
}: {
  source: string;
  language: string;
  label: string;
}) {
  const grammar = Prism.languages[language];
  const tokens = grammar ? Prism.tokenize(source, grammar) : source;
  return <CodeSurface label={label}>{prismContent(tokens, language)}</CodeSurface>;
}

type TokenLine = readonly { content: string; offset: number; className: string }[];
type HighlightState =
  | { kind: "plain" }
  | { kind: "ready"; source: string; language: string; lines: readonly TokenLine[] };

function scopeClass(scopes: readonly string[]) {
  if (scopes.some((scope) => scope.includes("comment"))) {
    return "text-inverse-muted";
  }
  if (scopes.some((scope) => scope.includes("string"))) {
    return "text-mint";
  }
  if (scopes.some((scope) => scope.includes("constant.numeric"))) {
    return "text-butter";
  }
  if (scopes.some((scope) => /keyword|storage/u.test(scope))) {
    return "text-inverse-link";
  }
  if (scopes.some((scope) => scope.includes("entity.name"))) {
    return "text-lilac";
  }
  return "text-inverse-text";
}

/** Highlight only settled code. Cancellation prevents an earlier source replacing a newer one. */
export function ShikiHighlighter({
  source,
  language,
  label,
  streaming = false,
}: {
  source: string;
  language: BundledLanguage | SpecialLanguage;
  label: string;
  streaming?: boolean;
}) {
  const [state, setState] = useState<HighlightState>({ kind: "plain" });
  useEffect(() => {
    let cancelled = false;
    if (streaming) {
      return () => {
        cancelled = true;
      };
    }
    import("shiki")
      .then(
        async ({ codeToTokens }) =>
          await codeToTokens(source, {
            lang: language,
            theme: "github-dark",
            includeExplanation: "scopeName",
          }),
      )
      .then((result) => {
        if (!cancelled) {
          setState({
            kind: "ready",
            source,
            language,
            lines: result.tokens.map((line) =>
              line.map((token) => ({
                content: token.content,
                offset: token.offset,
                className: scopeClass(
                  token.explanation?.flatMap((entry) =>
                    entry.scopes.map((scope) => scope.scopeName),
                  ) ?? [],
                ),
              })),
            ),
          });
        }
      })
      .catch(() => {
        if (!cancelled) {
          setState({ kind: "plain" });
        }
      });
    return () => {
      cancelled = true;
    };
  }, [source, language, streaming]);
  if (
    state.kind === "plain" ||
    state.source !== source ||
    state.language !== language ||
    streaming
  ) {
    return <CodeSurface label={label}>{source}</CodeSurface>;
  }
  return (
    <CodeSurface label={label}>
      {state.lines.map((line, lineNumber) => (
        // oxlint-disable-next-line react/no-array-index-key -- Line position is the identity of an immutable highlighted source snapshot, including empty lines.
        <span key={lineNumber} className="block min-h-5">
          {line.map((token) => (
            <span key={token.offset} className={token.className}>
              {token.content}
            </span>
          ))}
        </span>
      ))}
    </CodeSurface>
  );
}

type CodeLabels = { copy: string; copied: string; error: string };

const defaultCodeLabels: CodeLabels = {
  copy: "Copy code",
  copied: "Code copied",
  error: "Could not copy code",
};
const CodeLabelContext = createContext(defaultCodeLabels);

function MarkdownCodeBlock({ children }: { children?: ReactNode }) {
  const labels = useContext(CodeLabelContext);
  const [receipt, setReceipt] = useState<"idle" | "copied" | "error">("idle");
  const generation = useRef<{ source: string; token: symbol } | undefined>(undefined);
  const code = isValidElement<{ children?: ReactNode; className?: string }>(children)
    ? children
    : undefined;
  const source = typeof code?.props.children === "string" ? code.props.children : "";
  const language = code?.props.className?.replace(/^language-/u, "") ?? "";
  useEffect(() => {
    generation.current = { source, token: Symbol("markdown-source") };
    return () => {
      generation.current = undefined;
    };
  }, [source]);
  useEffect(() => {
    if (receipt === "idle") {
      return undefined;
    }
    const timer = setTimeout(() => {
      setReceipt("idle");
    }, 2000);
    return () => {
      clearTimeout(timer);
    };
  }, [receipt]);
  return (
    <div className="overflow-hidden rounded-control bg-inverse text-inverse-text">
      <div className="flex items-center justify-between gap-3 px-3 py-2">
        <span className="text-xs">{language}</span>
        <Button
          variant="ghost"
          size="sm"
          aria-label={labels.copy}
          onClick={() => {
            const { current } = generation;
            Promise.resolve()
              .then(async () => {
                await navigator.clipboard.writeText(source);
              })
              .then(() => {
                if (current === generation.current) {
                  setReceipt("copied");
                }
              })
              .catch(() => {
                if (current === generation.current) {
                  setReceipt("error");
                }
              });
          }}
        >
          <Icon
            name={receipt === "copied" ? "check" : "copy"}
            className={
              receipt === "copied"
                ? "animate-assistant-copy-check motion-reduce:animate-none"
                : "animate-assistant-copy-icon motion-reduce:animate-none"
            }
          />
        </Button>
        <output className="sr-only" aria-live="polite">
          {{ idle: "", copied: labels.copied, error: labels.error }[receipt]}
        </output>
      </div>
      <pre>{children}</pre>
    </div>
  );
}

const defaultMarkdownComponents: Components = {
  input: ({ checked, disabled }) => (
    <SelectionControl checked={checked} disabled={disabled} type="checkbox" />
  ),
  pre: MarkdownCodeBlock,
};

/** Raw HTML stays disabled. ReactMarkdown validates link protocols and renders semantic nodes. */
export function MarkdownText({
  text,
  className,
  components,
  codeLabels = defaultCodeLabels,
}: {
  text: string;
  className?: string;
  components?: Components;
  codeLabels?: CodeLabels;
}) {
  const deferredText = useDeferredValue(text);
  const markdownComponents = { ...defaultMarkdownComponents, ...components };
  return (
    <div
      className={cn(
        "max-w-prose space-y-3 text-content leading-content break-words [&_a]:text-primary [&_a]:underline [&_blockquote]:rounded-control [&_blockquote]:bg-surface-muted [&_blockquote]:p-4 [&_code]:font-mono [&_code]:text-code [&_h1]:text-document [&_h1]:font-semibold [&_h2]:text-title [&_h2]:font-medium [&_h3]:text-body [&_h3]:font-medium [&_li]:my-1 [&_ol]:list-decimal [&_ol]:pl-6 [&_pre]:overflow-x-auto [&_pre]:rounded-control [&_pre]:bg-inverse [&_pre]:p-4 [&_pre]:text-inverse-text [&_table]:w-full [&_td]:border [&_td]:border-border [&_td]:p-2 [&_th]:border [&_th]:border-border [&_th]:p-2 [&_ul]:list-disc [&_ul]:pl-6",
        className,
      )}
    >
      <CodeLabelContext value={codeLabels}>
        <ReactMarkdown remarkPlugins={[remarkGfm]} skipHtml components={markdownComponents}>
          {deferredText}
        </ReactMarkdown>
      </CodeLabelContext>
    </div>
  );
}

/** KaTeX receives text with trust disabled. Its generated MathML remains accessible. */
export function MathBlock({
  expression,
  label,
  steps,
  visibleSteps = steps?.length ?? 0,
}: {
  expression: string;
  label: string;
  steps?: readonly { id: string; expression: string; explanation: string }[];
  visibleSteps?: number;
}) {
  const html = useMemo(
    () =>
      renderToString(expression, {
        displayMode: true,
        throwOnError: false,
        trust: false,
        output: "htmlAndMathml",
      }),
    [expression],
  );
  return (
    <section aria-label={label} className="space-y-3">
      <div className="overflow-x-auto">
        {/* oxlint-disable-next-line react/no-danger -- KaTeX generates this markup from text with trust disabled; arbitrary HTML is never accepted. */}
        <div dangerouslySetInnerHTML={{ __html: html }} />
      </div>
      {steps ? (
        <ol className="space-y-3">
          {steps.slice(0, Math.max(0, visibleSteps)).map((step) => (
            <li key={step.id} className="animate-assistant-slide-up motion-reduce:animate-none">
              <MathBlock expression={step.expression} label={step.explanation} />
              <p className="text-ui text-text-muted">{step.explanation}</p>
            </li>
          ))}
        </ol>
      ) : null}
    </section>
  );
}

type DiagramState =
  | { kind: "pending" }
  | { kind: "ready"; source: string; src: string }
  | { kind: "invalid"; source: string };

const defaultDiagramControls = {
  zoomIn: "Zoom in",
  zoomOut: "Zoom out",
  reset: "Reset view",
  expand: "Expand diagram",
  close: "Close diagram",
};

/** Mermaid renders strict SVG into an image context; partial input keeps a source fallback. */
export function MermaidDiagram({
  source,
  label,
  pendingLabel,
  invalidLabel,
  streaming = false,
  controls = defaultDiagramControls,
}: {
  source: string;
  label: string;
  pendingLabel: string;
  invalidLabel: string;
  streaming?: boolean;
  controls?: { zoomIn: string; zoomOut: string; reset: string; expand: string; close: string };
}) {
  const id = useId().replaceAll(":", "");
  const ref = useRef<HTMLDivElement>(null);
  const [state, setState] = useState<DiagramState>({ kind: "pending" });
  const [lastValid, setLastValid] = useState<string>();
  const [open, setOpen] = useState(false);
  const [view, setView] = useState({ zoom: 1, x: 0, y: 0 });
  useEffect(() => {
    let cancelled = false;
    const element = ref.current;
    if (!element) {
      return undefined;
    }
    const style = getComputedStyle(element);
    import("mermaid")
      .then(async ({ default: mermaid }) => {
        mermaid.initialize({
          startOnLoad: false,
          securityLevel: "strict",
          suppressErrorRendering: true,
          theme: "base",
          themeVariables: {
            primaryColor: style.getPropertyValue("--color-primary-soft").trim(),
            primaryTextColor: style.getPropertyValue("--color-ink").trim(),
            primaryBorderColor: style.getPropertyValue("--color-primary").trim(),
            lineColor: style.getPropertyValue("--color-primary").trim(),
            fontFamily: "DM Sans, sans-serif",
          },
        });
        await mermaid.parse(source);
        return await mermaid.render(`swarmie-${id}`, source);
      })
      .then(({ svg }) => {
        if (!cancelled) {
          setLastValid(`data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`);
          setState({
            kind: "ready",
            source,
            src: `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`,
          });
        }
      })
      .catch(() => {
        if (!cancelled) {
          setState({ kind: "invalid", source });
        }
      });
    return () => {
      cancelled = true;
    };
  }, [id, source]);
  const imageSource = state.kind === "ready" ? state.src : lastValid;
  const pending = state.kind === "pending" || state.source !== source || streaming;
  const zoom = (delta: number) => {
    setView((current) => ({ ...current, zoom: Math.max(0.5, Math.min(4, current.zoom + delta)) }));
  };
  const toolbar = (
    <div className="flex flex-wrap items-center gap-2">
      <Button
        variant="ghost"
        onClick={() => {
          zoom(-0.25);
        }}
        disabled={view.zoom <= 0.5}
      >
        {controls.zoomOut}
      </Button>
      <output className="text-xs tabular-nums">{Math.round(view.zoom * 100)}%</output>
      <Button
        variant="ghost"
        onClick={() => {
          zoom(0.25);
        }}
        disabled={view.zoom >= 4}
      >
        {controls.zoomIn}
      </Button>
      <Button
        variant="secondary"
        onClick={() => {
          setView({ zoom: 1, x: 0, y: 0 });
        }}
      >
        {controls.reset}
      </Button>
    </div>
  );
  const diagram = (
    <DiagramViewport label={label} source={imageSource} view={view} onViewChange={setView} />
  );
  return (
    <div ref={ref} className="space-y-3">
      {imageSource === undefined ? (
        pending && (
          <div className="h-32 animate-pulse rounded-control bg-surface-muted motion-reduce:animate-none" />
        )
      ) : (
        <>
          {toolbar}
          <Button
            variant="ghost"
            onClick={() => {
              setOpen(true);
            }}
          >
            {controls.expand}
          </Button>
          {diagram}
        </>
      )}
      {pending || state.kind === "invalid" ? (
        <output className="block text-ui text-text-muted">
          {pending ? pendingLabel : invalidLabel}
        </output>
      ) : null}
      {imageSource === undefined && !pending ? (
        <CodeSurface label={label}>{source}</CodeSurface>
      ) : null}
      <AssistantModal open={open} onOpenChange={setOpen} title={label} closeLabel={controls.close}>
        <div className="animate-assistant-overlay-in motion-reduce:animate-none">
          {toolbar}
          {diagram}
        </div>
      </AssistantModal>
    </div>
  );
}
