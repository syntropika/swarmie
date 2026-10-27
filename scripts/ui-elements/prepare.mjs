import { createHash } from "node:crypto";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import nodePath from "node:path";

/** Reproduce the runtime-neutral Elements port from the licensed, immutable upstream snapshot. */
import { parse } from "@babel/parser";
import { format } from "oxfmt";

import formatOptions from "../../oxfmt.config.ts";

const { dirname, relative, resolve } = nodePath;
const root = resolve(import.meta.dirname, "../..");
const vendor = resolve(root, "packages/ui/web/vendor/assistant-elements");
const output = resolve(root, "packages/ui/web/target/assistant-elements");
const owned = resolve(root, "packages/ui/web/src/elements");
const manifest = JSON.parse(await readFile(resolve(vendor, "provenance.json"), "utf-8"));
const sources = new Map();
await Promise.all(
  Object.entries(manifest.files).map(async ([file, expected]) => {
    const content = await readFile(resolve(vendor, file), "utf-8");
    if (createHash("sha256").update(content).digest("hex") !== expected) {
      throw new Error(`Upstream snapshot integrity differs: ${file}`);
    }
    sources.set(file, content);
  }),
);
const writes = [];
async function writeGenerated(file, content) {
  await mkdir(dirname(file), { recursive: true });
  await writeFile(file, content);
}
const componentPrefix = "packages/ui/src/components/react/assistant-ui/";
const demoPrefix = "apps/docs/components/demo/";
const overrides = new Map([
  ["elements/markdown-text", "markdown"],
  ["elements/syntax-highlighter", "syntax-highlighter"],
  ["elements/tooltip-icon-button", "tooltip-icon-button"],
  ["elements/collapsible-root", "disclosure"],
  ["elements/inline-citation", "citation"],
  ["elements/conversation-map", "conversation-map"],
  ["elements/message-actions", "message-actions"],
  ["elements/thinking-indicator", "thinking-indicator"],
  ["elements/generative-ui", "generative-library"],
]);
function extension(path) {
  for (const suffix of ["", ".tsx", ".ts"]) {
    if (sources.has(path + suffix)) {
      return path + suffix;
    }
  }
  throw new Error(`Unresolved upstream source: ${path}`);
}
function relativeImport(from, to) {
  const path = relative(dirname(from), to)
    .replaceAll("\\", "/")
    .replace(/\.(?:tsx?|jsx?)$/u, "");
  return path.startsWith(".") ? path : `./${path}`;
}
const generated = new Map();
const pending = [];
function resolveImport(specifier, source) {
  if (specifier.startsWith("@/components/assistant-ui/")) {
    const name = specifier.slice("@/components/assistant-ui/".length);
    if (overrides.has(name)) {
      return { owned: overrides.get(name) };
    }
    return { source: extension(componentPrefix + name) };
  }
  if (specifier === "@/lib/utils") {
    return { owned: "utilities" };
  }
  if (specifier.startsWith("@/components/demo/")) {
    return { source: extension(demoPrefix + specifier.slice("@/components/demo/".length)) };
  }
  if (specifier.startsWith("@/hooks/")) {
    return { source: extension(`packages/ui/src/hooks/${specifier.slice("@/hooks/".length)}`) };
  }
  if (specifier.startsWith("@/lib/")) {
    return { source: extension(`apps/docs/lib/${specifier.slice("@/lib/".length)}`) };
  }
  if (specifier.startsWith("@/components/ui/")) {
    return { owned: `primitives/${specifier.slice("@/components/ui/".length)}` };
  }
  if (specifier.startsWith(".")) {
    const path = nodePath.posix.join(nodePath.posix.dirname(source), specifier);
    if (path.startsWith(componentPrefix)) {
      const name = path.slice(componentPrefix.length).replace(/\.tsx?$/u, "");
      if (overrides.has(name)) {
        return { owned: overrides.get(name) };
      }
    }
    return { source: extension(path) };
  }
  if (specifier.startsWith("@base-ui/")) {
    throw new Error(`Unported widget dependency: ${source}: ${specifier}`);
  }
  if (specifier === "@assistant-ui/react" || specifier.startsWith("@assistant-ui/react/")) {
    return { owned: "view-types" };
  }
  if (specifier.startsWith("@assistant-ui/react-")) {
    throw new Error(`Assistant runtime renderer dependency: ${source}: ${specifier}`);
  }
  return { external: specifier };
}
function schedule(source) {
  const destination = resolve(output, source);
  if (!generated.has(source)) {
    generated.set(source, destination);
    pending.push(source);
  }
  return destination;
}
function walk(node, visit) {
  if (!node || typeof node !== "object") {
    return;
  }
  visit(node);
  for (const value of Object.values(node)) {
    if (Array.isArray(value)) {
      for (const child of value) {
        walk(child, visit);
      }
    } else if (value && typeof value === "object" && typeof value.type === "string") {
      walk(value, visit);
    }
  }
}
function port(source, upstream) {
  let content = upstream;
  if (source.endsWith("/question-flow.tsx")) {
    content = content.replace(
      'role="progressbar"',
      'role="progressbar" aria-label="Question progress"',
    );
  }
  // The replacements contain literal TSX template expressions for the generated source.
  if (source.endsWith("/launcher-bubble.tsx")) {
    content = content.replace(
      'aria-label={open ? "Close the assistant" : "Open the assistant"}',
      // oxlint-disable-next-line no-template-curly-in-string -- Literal generated TSX.
      'aria-label={open ? "Close the assistant" : `Open the assistant${unread > 0 ? `, ${unread} unread` : ""}`}',
    );
  }
  if (source.endsWith("/image-gallery.tsx")) {
    content = content.replace(
      // oxlint-disable-next-line no-template-curly-in-string -- Literal generated TSX.
      "aria-label={`Open image: ${image.alt}`}",
      // oxlint-disable-next-line no-template-curly-in-string -- Literal generated TSX.
      'aria-label={`Open image: ${image.alt}${overflow ? `, +${images.length - visibleCount} more images` : ""}`}',
    );
  }

  const destination = generated.get(source);
  const ast = parse(content, { sourceType: "module", plugins: ["typescript", "jsx"] });
  const edits = [];
  const controls = new Set();
  walk(ast, (node) => {
    if (
      node.type === "ImportDeclaration" ||
      (node.type === "ExportNamedDeclaration" && node.source)
    ) {
      const specifier = node.source.value;
      const dependency = resolveImport(specifier, source);
      let target = dependency.external;
      if (dependency.owned) {
        target = relativeImport(destination, resolve(owned, dependency.owned));
      } else if (dependency.source) {
        target = relativeImport(destination, schedule(dependency.source));
      }
      edits.push({ start: node.source.start, end: node.source.end, text: JSON.stringify(target) });
    }
    if (node.type === "JSXOpeningElement") {
      const attributes = node.attributes.filter((attribute) => attribute.type === "JSXAttribute");
      const className = attributes.find((attribute) => attribute.name.name === "className");
      const classes = className ? content.slice(className.start, className.end) : "";
      if (/overflow-(?:x-auto|auto)|codeScroll/u.test(classes)) {
        const names = new Set(attributes.map((attribute) => attribute.name.name));
        const additions = [];
        if (!names.has("tabIndex")) {
          additions.push("tabIndex={0}");
        }
        if (!names.has("role")) {
          additions.push('role="group"');
        }
        if (!names.has("aria-label") && !names.has("aria-labelledby")) {
          const label = nodePath.basename(source, ".tsx").replaceAll("-", " ");
          additions.push(`aria-label=${JSON.stringify(`${label} content`)}`);
        }
        if (additions.length) {
          edits.push({ start: node.name.end, end: node.name.end, text: ` ${additions.join(" ")}` });
        }
      }
    }
    if (node.type === "JSXIdentifier" && ["button", "input", "textarea"].includes(node.name)) {
      const parentTag = content.slice(node.start - 2, node.start);
      if (parentTag.endsWith("<") || parentTag === "</") {
        const control = {
          button: "ElementButton",
          input: "ElementInput",
          textarea: "ElementTextArea",
        }[node.name];
        controls.add(control);
        edits.push({ start: node.start, end: node.end, text: control });
      }
    }
  });
  let result = content;
  for (const edit of edits.toSorted((a, b) => b.start - a.start)) {
    result = result.slice(0, edit.start) + edit.text + result.slice(edit.end);
  }
  if (controls.size) {
    result = `import { ${[...controls].join(", ")} } from ${JSON.stringify(relativeImport(destination, resolve(owned, "controls")))};\n${result}`;
  }
  const tokens = {
    "text-muted-foreground": "text-foreground",
    "bg-muted": "bg-surface-muted",
    "bg-popover": "bg-surface",
    "text-popover-foreground": "text-foreground",
    "border-input": "border-control-border",
    "ring-ring": "ring-focus",
    "bg-secondary": "bg-surface-muted",
    "text-secondary-foreground": "text-foreground",
    "bg-card": "bg-surface",
    "text-card-foreground": "text-foreground",
  };
  for (const [from, to] of Object.entries(tokens)) {
    result = result.replaceAll(from, to);
  }
  // Opacity-based upstream text colors assume another surface palette; shared semantic text stays readable.
  result = result.replaceAll(/text-foreground\/(?:\[[^\]]+\]|\d+)/gu, "text-foreground");
  result = result.replaceAll(
    /text-(?:blue|sky|cyan|violet|purple|indigo|amber|yellow|orange)-\d+/gu,
    "text-foreground",
  );
  result = result.replaceAll(/text-(?:emerald|green|teal)-\d+/gu, "text-foreground");
  result = result.replaceAll(/text-(?:red|rose)-\d+/gu, "text-foreground");
  // Keep text labels readable on tinted surfaces and inactive presentation rows.
  result = result.replaceAll(/(?<![\w:-])opacity-(?:[1-9]\d?)(?![\w])/gu, "opacity-100");
  return `// Generated from the pinned MIT-licensed snapshot by scripts/ui-elements/prepare.mjs.\n${
    result
  }`;
}
// Props-only components are the public UI surface. Runtime-bound .aui files are never copied into the port.
const publicEntries = [];
for (const source of [...sources.keys()].toSorted()) {
  if (
    !source.startsWith(`${componentPrefix}elements/`) ||
    !source.endsWith(".tsx") ||
    source.includes(".aui.") ||
    source.includes(".radix.")
  ) {
    continue;
  }
  const name = source.slice(componentPrefix.length).replace(/\.tsx$/u, "");
  if (
    [
      "elements/model-selector",
      "elements/generative-ui",
      "elements/markdown-text",
      "elements/syntax-highlighter",
    ].includes(name)
  ) {
    continue;
  }
  if (overrides.has(name)) {
    publicEntries.push({ source, owned: overrides.get(name) });
    continue;
  }
  schedule(source);
  publicEntries.push({ source });
}
for (const source of [...sources.keys()].toSorted()) {
  if (
    source.startsWith(`${demoPrefix}elements/`) &&
    source.endsWith(".tsx") &&
    !["aui-demos.tsx", "generative-demos.tsx", "run-activity.tsx"].some((name) =>
      source.endsWith(`/${name}`),
    )
  ) {
    schedule(source);
  }
}
// The host-owned demos replace every upstream runtime binding with presentation callbacks.
for (const source of [
  "apps/docs/lib/gallery-templates.ts",
  "apps/docs/lib/generative-elements.ts",
]) {
  schedule(source);
}
while (pending.length) {
  const source = pending.shift();
  const destination = generated.get(source);
  const content = port(source, sources.get(source));
  writes.push(writeGenerated(destination, content));
}
const exports = [];
for (const entry of publicEntries) {
  const content = sources.get(entry.source);
  const names = [
    ...content.matchAll(/export\s+(?:const|function|class|type|interface)\s+(?<name>\w+)/gu),
  ].map((m) => ({ name: m.groups.name, type: /^export\s+(?:type|interface)/u.test(m[0]) }));
  if (!names.length) {
    continue;
  }
  const target = entry.owned ? resolve(owned, entry.owned) : generated.get(entry.source);
  exports.push(
    `export { ${names.map((n) => (n.type ? "type " : "") + n.name).join(", ")} } from ${JSON.stringify(relativeImport(resolve(output, "index.ts"), target))};`,
  );
}
await writeFile(resolve(output, "index.ts"), `${exports.join("\n")}\n`);
await writeFile(
  resolve(output, "provenance.json"),
  `${JSON.stringify(
    {
      revision: manifest.revision,
      sourceFiles: generated.size,
      controls: "HeroUI via shared Swarmie controls",
      runtime: "host-owned data and callbacks; no assistant-ui runtime binding",
    },
    null,
    2,
  )}\n`,
);
console.log(`Prepared ${generated.size} runtime-neutral Elements source modules.`);
// Generate one addressable story per official catalog entry, with named variants.
const catalog = JSON.parse(
  await readFile(resolve(root, "scripts/ui-elements/catalog.json"), "utf-8"),
);
const demoSymbols = new Map();
for (const [source, content] of sources) {
  if (
    !source.startsWith(`${demoPrefix}elements/`) ||
    source.endsWith("/aui-demos.tsx") ||
    source.endsWith("/generative-demos.tsx") ||
    source.endsWith("/run-activity.tsx")
  ) {
    continue;
  }
  for (const match of content.matchAll(/export function (?<name>\w+)/gu)) {
    demoSymbols.set(match.groups.name, source);
  }
}
const ports = {
  AuiThreadDemo: "ThreadPortDemo",
  AuiAssistantModalDemo: "ModalPortDemo",
  AuiAssistantSidebarDemo: "SidebarPortDemo",
  AuiThreadListDemo: "ThreadListDemo",
  AuiThreadListSidebarDemo: "SidebarPortDemo",
  AuiVoiceDemo: "VoiceConversationDemo",
  AuiVoiceStatesDemo: "VoiceConversationDemo",
  AuiVoicePalettesDemo: "VoiceConversationDemo",
  AuiReasoningDemo: "ReasoningPanelDemo",
  AuiMessageTimingDemo: "MessageTimingDemo",
  AuiConversationMapDemo: "ConversationMapDemo",
  AuiContextDisplayDemo: "ContextBreakdownDemo",
  AuiMcpConfigDemo: "McpServerPanelDemo",
  AuiAttachmentDemo: "MessageAttachmentDemo",
  AuiFollowUpSuggestionsDemo: "SuggestionsDemo",
  AuiToolFallbackDemo: "ToolCallDemo",
  AuiToolFallbackReceiptDemo: "ToolCallDemo",
  AuiToolGroupDemo: "ToolGroupDemo",
  AuiQuoteDemo: "QuoteReplyDemo",
  AuiSourcesDemo: "SourcesDemo",
  AuiSourcesVariantsDemo: "SourcesDemo",
  AuiImageDemo: "ImagePortDemo",
  AuiFileDemo: "FilePortDemo",
  AuiModelSelectorDemo: "ModelSelectorPortDemo",
  AuiComposerTriggerPopoverDemo: "ComposerSlashDemo",
  AuiDirectiveTextDemo: "ComposerMentionsDemo",
  AuiMarkdownTextDemo: "MarkdownPortDemo",
  AuiSyntaxHighlighterDemo: "SyntaxPortDemo",
  AuiShikiHighlighterDemo: "SyntaxPortDemo",
  AuiMermaidDiagramDemo: "MermaidPortDemo",
  AuiGenerativeUIDemo: "GenerativePortDemo",
  AuiTooltipIconButtonDemo: "TooltipPortDemo",
  AuiLogosDemo: "LogosPortDemo",
  AuiHeatGraphDemo: "HeatGraphPortDemo",
  RunActivityDemo: "RunActivityPortDemo",
};
let stories = 0;
for (const section of catalog) {
  for (const entry of section.elements) {
    const path = resolve(output, "catalog", `${entry.slug}.stories.tsx`);
    const imports = [
      `import type { Meta, StoryObj } from '@storybook/react-vite';`,
      `import { ElementCatalogFrame } from ${JSON.stringify(relativeImport(path, resolve(root, "packages/ui/web/.storybook/elements/frame")))};`,
    ];
    const used = new Set();
    const render = (symbol) => {
      if (symbol.startsWith("Generative")) {
        if (!used.has("GenerativePortDemo")) {
          imports.push(
            `import { GenerativePortDemo } from ${JSON.stringify(relativeImport(path, resolve(root, "packages/ui/web/.storybook/elements/ports")))};`,
          );
          used.add("GenerativePortDemo");
        }
        return `<GenerativePortDemo slug=${JSON.stringify(entry.slug.replace(/^generative-/u, ""))} />`;
      }
      if (ports[symbol]) {
        const name = ports[symbol];
        if (!used.has(name)) {
          imports.push(
            `import { ${name} } from ${JSON.stringify(relativeImport(path, resolve(root, "packages/ui/web/.storybook/elements/ports")))};`,
          );
          used.add(name);
        }
        return `<${name} />`;
      }
      const source = demoSymbols.get(symbol);
      if (!source) {
        throw new Error(`Catalog component missing a runtime-neutral port: ${symbol}`);
      }
      if (!used.has(symbol)) {
        imports.push(
          `import { ${symbol} } from ${JSON.stringify(relativeImport(path, generated.get(source)))};`,
        );
        used.add(symbol);
      }
      return `<${symbol} />`;
    };
    const variants = entry.variants ?? [
      { key: "default", label: "Default", Component: entry.Component },
    ];
    const declarations = variants.map((variant) => {
      const name = variant.key
        .split(/[^a-zA-Z0-9]/u)
        .map((part) => part.charAt(0).toUpperCase() + part.slice(1))
        .join("");
      const label = variant.key === "runtime" ? "Host-owned state" : variant.label;
      return `export const ${name}:Story={ name:${JSON.stringify(label)},render:()=> <ElementCatalogFrame>${render(variant.Component)}</ElementCatalogFrame>};`;
    });
    const category =
      section.label === "AUI connected" ? "Runtime-neutral compositions" : section.label;
    const description = `${
      entry.description +
      (entry.connection
        ? " Host-owned data and callbacks replace the upstream assistant-ui runtime binding."
        : "")
    } Source: https://www.assistant-ui.com/elements/${entry.slug}`;
    const code = `${imports.join(
      "\n",
    )}\nconst meta={title:${JSON.stringify(`Assistant Elements/${category}/${entry.title}`)},parameters:{layout:'padded',docs:{description:{component:${JSON.stringify(description)}}}}} satisfies Meta;export default meta;type Story=StoryObj<typeof meta>;\n${declarations.join(
      "\n",
    )}\n`;
    writes.push(writeGenerated(path, code));
    stories += variants.length;
  }
}
// Flat, explicit public exports retain unique upstream names. Helper collisions remain module-local.
const names = new Set();
const facade = [];
for (const entry of publicEntries) {
  const ast = parse(sources.get(entry.source), {
    sourceType: "module",
    plugins: ["typescript", "jsx"],
  });
  const symbols = [];
  for (const node of ast.program.body) {
    if (node.type !== "ExportNamedDeclaration") {
      continue;
    }
    if (node.declaration) {
      const { declaration } = node;
      if (declaration.id) {
        symbols.push({
          name: declaration.id.name,
          type: ["TSTypeAliasDeclaration", "TSInterfaceDeclaration"].includes(declaration.type),
        });
      }
      if (declaration.declarations) {
        for (const item of declaration.declarations) {
          if (item.id.type === "Identifier") {
            symbols.push({ name: item.id.name, type: false });
          }
        }
      }
    }
    for (const specifier of node.specifiers) {
      if (specifier.exported.type === "Identifier") {
        symbols.push({
          name: specifier.exported.name,
          type: node.exportKind === "type" || specifier.exportKind === "type",
        });
      }
    }
  }
  const selected = symbols.filter((symbol) => {
    if (names.has(symbol.name)) {
      return false;
    }
    names.add(symbol.name);
    return true;
  });
  if (!selected.length) {
    continue;
  }
  const target = entry.owned ? resolve(owned, entry.owned) : generated.get(entry.source);
  facade.push(
    `export { ${selected.map((symbol) => (symbol.type ? "type " : "") + symbol.name).join(", ")} } from ${JSON.stringify(relativeImport(resolve(owned, "index.ts"), target))};`,
  );
}
facade.push(
  'export { ConversationThread, type ConversationThreadProps, type ConversationMessageView } from "./conversation";',
  'export { MarkdownText } from "./markdown";',
  'export { ModelSelector, type ModelSelectorProps, type ModelOptionView } from "./model-selector";',
  'export { ComposerPanel } from "./composer-panel";',
  'export { GenerativePresentation, type GenerativePresentationProps } from "./generative";',
);

await Promise.all(writes);
const facadeSource = `// Generated public presentation exports; the source revision and transforms live in scripts/ui-elements.\n${facade.join("\n")}\n`;
const formatted = await format(resolve(owned, "index.ts"), facadeSource, formatOptions);
if (formatted.errors.length) {
  throw new Error("Cannot format the generated public Elements interface");
}
await writeFile(resolve(owned, "index.ts"), formatted.code);
console.log(
  `Prepared ${catalog.flatMap((section) => section.elements).length} official Elements entries and ${stories} stories. No upstream .aui runtime binding is included.`,
);
