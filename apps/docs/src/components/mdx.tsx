import defaultMdxComponents from "fumadocs-ui/mdx";
import type { MDXComponents } from "mdx/types";

import { AnnotatedCapture, SlotIndex } from "~/src/components/extension-guide";

export function getMDXComponents(components?: MDXComponents) {
  return {
    ...defaultMdxComponents,
    AnnotatedCapture,
    SlotIndex,
    ...components,
  } satisfies MDXComponents;
}

export const useMDXComponents = getMDXComponents;

declare global {
  type MDXProvidedComponents = ReturnType<typeof getMDXComponents>;
}
