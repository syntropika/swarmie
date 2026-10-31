import type { Snapshot } from "@swarmie/client-ui";
import type { ComponentType } from "react";

export type SectionProps = { params: Readonly<Record<string, string>> };
export type ApplicationSection = {
  id: string;
  owner: string;
  label: string;
  icon: ComponentType<{ className?: string }>;
  component: ComponentType<SectionProps>;
  header: ComponentType<SectionProps>;
  overlay?: ComponentType;
};
export type ShellUiApi = {
  sections: Snapshot<readonly ApplicationSection[]>;
  registerSection: (section: ApplicationSection) => () => void;
  dispose: () => void;
};

/** Shell owns application destinations; contributors retain their screen implementation. */
export function createShellUi(): ShellUiApi {
  let sections: readonly ApplicationSection[] = [];
  let disposed = false;
  const listeners = new Set<() => void>();
  function publish() {
    for (const changed of listeners) {
      changed();
    }
  }
  return {
    sections: {
      getSnapshot: () => sections,
      subscribe(changed) {
        listeners.add(changed);
        return () => {
          listeners.delete(changed);
        };
      },
    },
    registerSection(section) {
      if (disposed) {
        throw new Error("Shell is no longer active.");
      }
      if (sections.some((entry) => entry.id === section.id)) {
        throw new Error("Duplicate application section.");
      }
      sections = [...sections, section].toSorted((a, b) => a.id.localeCompare(b.id));
      publish();
      return () => {
        if (!sections.includes(section)) {
          return;
        }
        sections = sections.filter((entry) => entry !== section);
        publish();
      };
    },
    dispose() {
      disposed = true;
      sections = [];
      publish();
      listeners.clear();
    },
  };
}
