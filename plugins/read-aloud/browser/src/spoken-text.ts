import type { RootContent, Root } from "mdast";
import remarkGfm from "remark-gfm";
import remarkParse from "remark-parse";
import { unified } from "unified";

const markdown = unified().use(remarkParse).use(remarkGfm);

/** Read prose, never Markdown delimiters, code, link targets or hidden message parts. */
export function spokenText(content: unknown): string {
  const source = typeof content === "string" ? content : textParts(content);
  return prose(markdown.parse(source))
    .replaceAll(/[\t ]+/gu, " ")
    .replaceAll(/ *\n */gu, "\n")
    .replaceAll(/\n{3,}/gu, "\n\n")
    .trim();
}
function textParts(content: unknown): string {
  if (!Array.isArray(content)) {
    return "";
  }
  return content
    .filter(
      (part: unknown): part is { readonly type: "text"; readonly text: string } =>
        typeof part === "object" &&
        part !== null &&
        "type" in part &&
        part.type === "text" &&
        "text" in part &&
        typeof part.text === "string",
    )
    .map((part) => part.text)
    .join("\n\n");
}
const omitted = new Set<string>([
  "code",
  "inlineCode",
  "definition",
  "footnoteDefinition",
  "footnoteReference",
  "html",
  "thematicBreak",
  "yaml",
]);
const inline = new Set<string>([
  "paragraph",
  "heading",
  "emphasis",
  "strong",
  "delete",
  "link",
  "linkReference",
  "tableCell",
]);
function prose(node: Root | RootContent): string {
  if (omitted.has(node.type)) {
    return "";
  }
  if (node.type === "text") {
    return node.value;
  }
  if (node.type === "break") {
    return "\n";
  }
  if (node.type === "image" || node.type === "imageReference") {
    return node.alt ?? "";
  }
  if (!("children" in node)) {
    return "";
  }
  const children = node.children.map(prose).filter((text) => text !== "");
  if (node.type === "tableRow") {
    return children.join(". ");
  }
  return children.join(inline.has(node.type) ? "" : "\n\n");
}
