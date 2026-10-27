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
      className={`flex w-10.5 shrink-0 flex-col items-center border-r border-border py-2 ${hidden ? "hidden md:flex" : ""}`}
    >
      <button
        type="button"
        aria-label="Swarmie conversations"
        onClick={() => {
          navigate({ section: "conversations" });
        }}
        className="mb-6 size-7"
      >
        <BrandMark className="size-7 [&_img]:size-7" />
      </button>
      <IconButton
        size="rail"
        icon={MessageCircle}
        label="Conversations"
        active={sectionId === "conversations"}
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
