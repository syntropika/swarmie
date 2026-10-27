import type { ComponentProps, PropsWithChildren } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";

import { Typography } from "../atoms/typography";

function CodeBlock({ children }: PropsWithChildren) {
  return (
    // oxlint-disable-next-line jsx-a11y/no-noninteractive-tabindex, jsx-a11y/prefer-tag-over-role -- Scrollable code needs a named keyboard-focusable region while retaining preformatted semantics.
    <pre tabIndex={0} role="region" aria-label="Code block">
      {children}
    </pre>
  );
}
const components = { pre: CodeBlock };
export function MarkdownText({ text, className }: { text: string; className?: string }) {
  return (
    <Typography.Prose {...(className === undefined ? {} : { className })}>
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
