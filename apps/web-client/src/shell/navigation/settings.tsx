import { useSource } from "@swarmie/client-ui";
import { ChromeButton, SearchField } from "@swarmie/ui-web";
import { GitBranch, LockKeyhole, MessageCircle, Monitor, Puzzle, Settings } from "lucide-react";
import { useState } from "react";

import { useClient } from "../../client/context";
import { useSelection } from "../../client/use-selection";
import { iconStyle } from "../../controls";
import { shellSettings } from "./state";

const sections = [
  { id: "vault", icon: LockKeyhole, title: "Vault", terms: "secrets credentials access" },
  { id: "general", icon: Settings, title: "General", terms: "appearance theme system dark light" },
  {
    id: "conversation",
    icon: MessageCircle,
    title: "Conversation",
    terms: "chat tool tools results activity display grouped separate hidden thinking",
  },
  { id: "connections", icon: Monitor, title: "Connections", terms: "nodes connect url user" },
  { id: "git", icon: GitBranch, title: "Git", terms: "repository worktree changes setup script" },
  { id: "providers", icon: Puzzle, title: "Providers", terms: "models accounts" },
  { id: "plugins", icon: Puzzle, title: "Plugins", terms: "integrations mcp servers" },
] as const;

export function SettingsNavigation() {
  const { navigate, shell, shellState, gitPresentation } = useClient();
  const git = useSource(gitPresentation.state);
  const gitAvailable = git.enabled && Object.values(git.nodes).some((node) => node.available);
  const selected = useSelection(shell, shellSettings);
  const [query, setQuery] = useState("");
  const terms = query.trim().toLocaleLowerCase().split(/\s+/u).filter(Boolean);
  const results = sections.filter((section) => {
    if (section.id === "git" && !gitAvailable) {
      return false;
    }
    const text = `${section.title} ${section.terms}`.toLocaleLowerCase();
    return terms.every((term) => text.includes(term));
  });
  return (
    <>
      <div className="flex h-(--spacing-navigation-search-header) shrink-0 items-center border-b border-navigation-divider bg-navigation-surface px-(--spacing-menu-inset) [--field-border-hover:transparent] [--field-border:transparent]">
        <SearchField
          aria-label="Search settings"
          value={query}
          onChange={setQuery}
          className="w-full min-w-0"
        >
          <SearchField.Group>
            <SearchField.SearchIcon />
            <SearchField.Input placeholder="Search settings" className="min-w-0" />
            <SearchField.ClearButton />
          </SearchField.Group>
        </SearchField>
      </div>
      <nav
        aria-label="Settings sections"
        className="min-h-0 flex-1 space-y-0.5 overflow-y-auto overscroll-contain p-(--spacing-menu-inset)"
      >
        {results.map(({ id, icon: Icon, title }) => (
          <ChromeButton
            key={id}
            size="navigationMenu"
            selected={selected === id}
            aria-current={selected === id ? "page" : false}
            onPress={() => {
              navigate({ settings: id, params: {} });
              if (window.innerWidth < 768) {
                shellState.update((s) => ({ ...s, navigation: false }));
              }
            }}
          >
            <Icon className={iconStyle} />
            {title}
          </ChromeButton>
        ))}
        {results.length === 0 ? (
          <output className="block px-(--spacing-menu-row-x) py-(--spacing-menu-row-y) text-sm text-muted">
            No settings found.
          </output>
        ) : null}
      </nav>
    </>
  );
}
