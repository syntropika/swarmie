import type { ComponentProps, ComponentPropsWithRef, ReactNode } from "react";

import { Disclosure } from "../atoms/disclosure";
import { definedProps } from "./defined-props";

export function CollapsibleRoot({
  open,
  defaultOpen,
  onOpenChange,
  children,
  ...props
}: Omit<ComponentPropsWithRef<"div">, "onChange"> & {
  open?: boolean | undefined;
  defaultOpen?: boolean | undefined;
  onOpenChange?: ((open: boolean) => void) | undefined;
  children?: ReactNode;
}) {
  return (
    <Disclosure
      {...definedProps(props)}
      {...(open === undefined ? {} : { isExpanded: open })}
      {...(defaultOpen === undefined ? {} : { defaultExpanded: defaultOpen })}
      {...(onOpenChange === undefined ? {} : { onExpandedChange: onOpenChange })}
    >
      {children}
    </Disclosure>
  );
}
export function CollapsibleTrigger(props: ComponentProps<typeof Disclosure.Trigger>) {
  return (
    <Disclosure.Heading>
      <Disclosure.Trigger {...props} />
    </Disclosure.Heading>
  );
}
export function CollapsibleContent(props: ComponentProps<typeof Disclosure.Content>) {
  return <Disclosure.Content {...props} />;
}
