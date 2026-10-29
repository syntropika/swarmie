import { useSource } from "@swarmie/client-ui";
import { BrandMark, ChromeButton } from "@swarmie/ui-web";
import { MessageCircle } from "lucide-react";

import { useClient } from "../../client/context";
import { useSelection } from "../../client/use-selection";
import { IconButton } from "../../controls";
import { AccountMenu } from "./account-menu";
import { NodeStatus } from "./node-status";
import { shellSection } from "./state";

export function NavigationRail({ hidden }: { hidden: boolean }) {
  const { navigate, shell, shellUi, shellState } = useClient();
  const sectionId = useSelection(shell, shellSection);
  const sections = useSource(shellUi.sections);
  return (
    <nav
      aria-label="Application"
      className={`flex min-h-0 w-(--spacing-shell-rail) shrink-0 flex-col items-center gap-(--spacing-chrome-rail-gap) overflow-x-hidden overflow-y-auto overscroll-contain bg-workspace pb-(--spacing-chrome-inset) ${hidden ? "hidden md:flex" : ""}`}
    >
      <div className="flex h-(--spacing-navigation-header) w-full shrink-0 items-center justify-center">
        <ChromeButton
          size="rail"
          appearance="bare"
          aria-label="Swarmie conversations"
          onPress={() => {
            navigate({ section: "conversations" });
            if (window.innerWidth < 768) {
              shellState.update((current) => ({ ...current, navigation: false }));
            }
          }}
        >
          <BrandMark className="size-(--spacing-brand-mark) [&_img]:size-(--spacing-brand-mark)" />
        </ChromeButton>
      </div>
      <IconButton
        size="rail"
        icon={MessageCircle}
        label="Conversations"
        active={sectionId === "conversations"}
        aria-current={sectionId === "conversations" ? "page" : false}
        onPress={() => {
          navigate({ section: "conversations" });
          if (window.innerWidth < 768) {
            shellState.update((current) => ({ ...current, navigation: false }));
          }
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
      <div
        data-slot="rail-utilities"
        className="mt-auto flex shrink-0 flex-col items-center gap-(--spacing-chrome-rail-gap)"
      >
        <NodeStatus />
        <AccountMenu />
      </div>
    </nav>
  );
}
