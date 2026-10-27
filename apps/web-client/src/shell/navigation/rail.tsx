import { useSource } from "@swarmie/client-ui";
import { BrandMark } from "@swarmie/ui-web";
import { MessageCircle } from "lucide-react";

import { useClient } from "../../client/context";
import { useSelection } from "../../client/use-selection";
import { IconButton } from "../../controls";
import { AccountMenu } from "./account-menu";
import { shellSection } from "./state";

export function NavigationRail({ hidden }: { hidden: boolean }) {
  const { navigate, shell, shellUi, shellState } = useClient();
  const sectionId = useSelection(shell, shellSection);
  const sections = useSource(shellUi.sections);
  return (
    <nav
      aria-label="Application"
      className={`flex w-(--spacing-shell-rail) shrink-0 flex-col items-center gap-(--spacing-chrome-rail-gap) border-r border-border py-2 md:pt-0 ${hidden ? "hidden md:flex" : ""}`}
    >
      <div className="flex h-(--spacing-chrome-rail-control) w-full shrink-0 items-center justify-center md:h-(--spacing-shell-rail)">
        <button
          type="button"
          aria-label="Swarmie conversations"
          onClick={() => {
            navigate({ section: "conversations" });
          }}
          className="flex size-(--spacing-chrome-rail-control) items-center justify-center"
        >
          <BrandMark className="size-(--spacing-brand-mark) [&_img]:size-(--spacing-brand-mark)" />
        </button>
      </div>
      <IconButton
        size="rail"
        icon={MessageCircle}
        label="Conversations"
        active={sectionId === "conversations"}
        aria-current={sectionId === "conversations" ? "page" : false}
        onPress={() => {
          navigate({ section: "conversations" });
        }}
      />
      {sections.map((section) => (
        <IconButton
          key={section.id}
          size="rail"
          icon={section.icon}
          label={section.label}
          active={sectionId === section.id}
          aria-current={sectionId === section.id ? "page" : false}
          onPress={() => {
            navigate({ section: section.id, params: {} });
            if (window.innerWidth < 768) {
              shellState.update((current) => ({ ...current, navigation: false }));
            }
          }}
        />
      ))}
      <div className="mt-auto">
        <AccountMenu />
      </div>
    </nav>
  );
}
