import { useEffect, useState } from "react";

export type CodeTone = "plain" | "keyword" | "function" | "type" | "string" | "literal" | "comment";
export type CodeToken = {
  text: string;
  offset: number;
  tone: CodeTone;
  italic: boolean;
  bold: boolean;
  underline: boolean;
};
export type CodeTokens = CodeToken[][];

/** Token data supports custom code layouts without rendering or executing HTML. */
export async function highlightCode(code: string, language: string): Promise<CodeTokens> {
  const { tokenize } = await import("./tokenize");
  return await tokenize(code, language);
}

export function codeLanguage(path: string): string {
  const name = path.split(/[\\/]/u).at(-1)?.toLowerCase() ?? "";
  if (name === "dockerfile") {
    return "dockerfile";
  }
  if (name === "makefile") {
    return "makefile";
  }
  const extension = name.split(".").at(-1) ?? "";
  const languages = {
    ts: "typescript",
    tsx: "tsx",
    js: "javascript",
    jsx: "jsx",
    mjs: "javascript",
    cjs: "javascript",
    rs: "rust",
    py: "python",
    go: "go",
    zig: "zig",
    json: "json",
    jsonc: "jsonc",
    css: "css",
    scss: "scss",
    html: "html",
    vue: "vue",
    svelte: "svelte",
    sh: "shellscript",
    bash: "shellscript",
    zsh: "shellscript",
    toml: "toml",
    yaml: "yaml",
    yml: "yaml",
    md: "markdown",
    mdx: "mdx",
    sql: "sql",
    c: "c",
    h: "c",
    cpp: "cpp",
    hpp: "cpp",
    cs: "csharp",
    java: "java",
    kt: "kotlin",
    rb: "ruby",
    php: "php",
    lua: "lua",
  };
  return Object.entries(languages).find(([suffix]) => suffix === extension)?.[1] ?? "text";
}

export function CodeLine({
  tokens,
  text,
}: {
  tokens: readonly CodeToken[] | undefined;
  text: string;
}) {
  if (tokens === undefined) {
    return text;
  }
  return tokens.map((token) => (
    <span
      key={token.offset}
      data-code-tone={token.tone}
      className={` ${token.italic ? "italic" : ""} ${token.bold ? "font-semibold" : ""} ${token.underline ? "underline" : ""}`}
    >
      {token.text}
    </span>
  ));
}

export function CodePreview({ code, path }: { code: string; path: string }) {
  const [result, setResult] = useState<{ code: string; path: string; tokens: CodeTokens } | null>(
    null,
  );
  useEffect(() => {
    let active = true;
    highlightCode(code, codeLanguage(path))
      .then((tokens) => {
        if (active) {
          setResult({ code, path, tokens });
        }
      })
      .catch(() => {
        if (active) {
          setResult(null);
        }
      });
    return () => {
      active = false;
    };
  }, [code, path]);
  const tokens = result?.code === code && result.path === path ? result.tokens : undefined;
  return (
    <pre
      className="min-h-0 flex-1 overflow-auto py-3 font-mono text-xs leading-6"
      aria-label="File content"
    >
      <code>
        {code
          .split("\n")
          .map((text, index) => ({ text, number: index + 1 }))
          .map((line) => (
            <span key={line.number} className="flex min-w-max">
              <span
                aria-hidden="true"
                className="w-14 shrink-0 pr-4 text-right text-muted tabular-nums select-none"
              >
                {line.number}
              </span>
              <span className="pr-5">
                <CodeLine tokens={tokens?.[line.number - 1]} text={line.text} />
                {"\n"}
              </span>
            </span>
          ))}
      </code>
    </pre>
  );
}
