import { createHighlighterCore } from "shiki/core";
import { createJavaScriptRegexEngine } from "shiki/engine/javascript";
import { bundledLanguages } from "shiki/langs";
import type { BundledLanguage } from "shiki/langs";
import dark from "shiki/themes/github-dark-default.mjs";
import light from "shiki/themes/github-light-default.mjs";

import type { CodeTokens, CodeTone } from "./index";

const highlighter = createHighlighterCore({
  themes: [light, dark],
  engine: createJavaScriptRegexEngine(),
});

function bundled(language: string): language is BundledLanguage {
  return Object.hasOwn(bundledLanguages, language);
}

function tone(color: string | undefined): CodeTone {
  switch (color?.toLowerCase()) {
    case undefined: {
      return "plain";
    }
    case "#cf222e": {
      return "keyword";
    }
    case "#8250df": {
      return "function";
    }
    case "#953800": {
      return "type";
    }
    case "#0a3069":
    case "#116329": {
      return "string";
    }
    case "#0550ae": {
      return "literal";
    }
    case "#6e7781": {
      return "comment";
    }
    default: {
      return "plain";
    }
  }
}

export async function tokenize(code: string, language: string): Promise<CodeTokens> {
  const engine = await highlighter;
  if (bundled(language) && !engine.getLoadedLanguages().includes(language)) {
    await engine.loadLanguage(await bundledLanguages[language]());
  }
  const lang = engine.getLoadedLanguages().includes(language) ? language : "text";
  const tokens = engine.codeToTokensWithThemes(code, {
    lang,
    themes: { light: light.name, dark: dark.name },
  });
  return tokens.map((line) =>
    line.map((token) => {
      const variant = token.variants.light;
      const font = variant?.fontStyle ?? 0;
      return {
        text: token.content,
        offset: token.offset,
        tone: tone(variant?.color),
        italic: font % 2 === 1,
        bold: Math.floor(font / 2) % 2 === 1,
        underline: Math.floor(font / 4) % 2 === 1,
      };
    }),
  );
}
