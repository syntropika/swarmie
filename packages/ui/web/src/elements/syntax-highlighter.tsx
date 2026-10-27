import { SyntaxHighlighter as ShikiHighlighter } from "../../target/assistant-elements/packages/ui/src/components/react/assistant-ui/elements/shiki-highlighter";

export function SyntaxHighlighter({
  code,
  language = "text",
}: {
  code: string;
  language?: string;
}) {
  return <ShikiHighlighter code={code} language={language} />;
}
