import type { ComponentProps, PropsWithChildren } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";

import { Typography } from "../atoms/typography";
import { cn } from "../classes";

function CodeBlock({ children }: PropsWithChildren) {
  /* oxlint-disable jsx-a11y/no-noninteractive-tabindex, jsx-a11y/prefer-tag-over-role -- Scrollable code needs a named keyboard-focusable region while retaining preformatted semantics. */
  return (
    <pre
      tabIndex={0}
      role="region"
      aria-label="Code block"
      className="focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-focus"
    >
      {children}
    </pre>
  );
  /* oxlint-enable jsx-a11y/no-noninteractive-tabindex, jsx-a11y/prefer-tag-over-role */
}
function MarkdownTable({ children }: ComponentProps<"table">) {
  /* oxlint-disable jsx-a11y/no-noninteractive-tabindex -- The named scroll region must accept focus for keyboard scrolling. */
  return (
    <section
      tabIndex={0}
      aria-label="Table"
      className="max-w-full overflow-x-auto focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-focus"
    >
      <table>{children}</table>
    </section>
  );
  /* oxlint-enable jsx-a11y/no-noninteractive-tabindex */
}
const components = { pre: CodeBlock, table: MarkdownTable };
export function MarkdownText({ text, className }: { text: string; className?: string }) {
  return (
    <Typography.Prose className={cn("min-w-0 break-words [&_img]:max-w-full", className)}>
      <ReactMarkdown remarkPlugins={[remarkGfm]} components={components}>
        {text}
      </ReactMarkdown>
    </Typography.Prose>
  );
}
export function MarkdownTextPrimitive({
  text = "",
  ...props
}: Omit<ComponentProps<typeof MarkdownText>, "text"> & { text?: string }) {
  return <MarkdownText text={text} {...props} />;
}
