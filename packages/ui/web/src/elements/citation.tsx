import type { ComponentProps } from "react";

import { Button } from "../atoms/button";
import { Link } from "../atoms/link";
import { Popover } from "../atoms/popover";

export type Source = {
  title: string;
  snippet: string;
  domain?: string | undefined;
  url?: string | undefined;
  publishedAt?: string | undefined;
};
export type CitationProps = {
  index: number;
  source: Source;
  open?: boolean | undefined;
  onOpenChange?: ((open: boolean) => void) | undefined;
} & Omit<ComponentProps<typeof Button>, "children" | "aria-label">;
export function Citation({ index, source, open, onOpenChange, ...props }: CitationProps) {
  const href =
    source.url !== undefined && /^https?:\/\//u.test(source.url) ? source.url : undefined;
  return (
    <Popover
      {...(open === undefined ? {} : { isOpen: open })}
      {...(onOpenChange === undefined ? {} : { onOpenChange })}
    >
      <Button {...props} variant="tertiary" aria-label={`Source ${index + 1}: ${source.title}`}>
        {index + 1}
      </Button>
      <Popover.Content className="w-70 max-w-[calc(100vw-24px)]">
        <Popover.Dialog className="min-w-0 break-words">
          <Popover.Heading>{source.title}</Popover.Heading>
          <p>{source.snippet}</p>
          {href === undefined ? null : <Link href={href}>{source.domain ?? "Open source"}</Link>}
        </Popover.Dialog>
      </Popover.Content>
    </Popover>
  );
}

export type InlineCitationProps = ComponentProps<"p">;
export function InlineCitation(props: InlineCitationProps) {
  return <p {...props} />;
}
