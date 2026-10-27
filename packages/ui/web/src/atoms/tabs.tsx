import { Tabs as HeroTabs } from "@heroui/react";
import type { TabListContainerProps as HeroTabListContainerProps } from "@heroui/react";
import type { ComponentProps } from "react";

import { cn } from "../classes";

type ListContainerProps = HeroTabListContainerProps & {
  appearance?: "default" | "filters";
};

function ListContainer({ appearance = "default", className, ...props }: ListContainerProps) {
  return (
    <HeroTabs.ListContainer
      {...props}
      className={cn(appearance === "filters" && "swarmie-filter-tabs", className)}
    />
  );
}

export type TabsProps = ComponentProps<typeof HeroTabs> & {
  appearance?: "default" | "view";
};

function Root({ appearance = "default", className, ...props }: TabsProps) {
  return (
    <HeroTabs {...props} className={cn(appearance === "view" && "swarmie-view-tabs", className)} />
  );
}

export const Tabs = Object.assign(Root, HeroTabs, { ListContainer });
