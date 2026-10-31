import { fixtureProjects } from "../client-ui/projects";
import { Button } from "@swarmie/ui-web";
import { ComposerPanel } from "@swarmie/ui-web/elements/conversation";
import { setThemePreference } from "@swarmie/ui-web/theme";
import { createHost } from "@syntropika/tembra-sdk/browser";
import { useState } from "react";
import { createRoot } from "react-dom/client";

import { initializeClient } from "../../../apps/web-client/src/client/initialize";
import { useSource } from "../../../packages/client-ui/src/index";
import { ProjectsContext } from "../../../plugins/projects/browser/src/context";
import { ProjectDialog } from "../../../plugins/projects/browser/src/editor";
import { ProjectSelector } from "../../../plugins/projects/browser/src/selector";

import "../../../apps/web-client/src/styles.css";

const projectsBackend = fixtureProjects({
  records: [
    { id: "website", name: "Website", description: "", archived: false },
    { id: "project:long", name: "A project with a long name that must stay inside the menu", description: "", archived: false },
    { id: "archived", name: "Archived project", description: "", archived: true },
  ],
  links: {"fixture-existing": "archived"}, personalFolders: [],
});
const client = await initializeClient(await createHost({name: "project-selector-fixture"}), {
  dev: true, projects: projectsBackend,
});
client.projectsState.update((current) => ({...current,...projectsBackend.snapshot(),available: true}));

function Fixture() {
  const state = useSource(client.projectsState);
  const [isDraft, setDraft] = useState(true);
  const [many, setMany] = useState(false);
  const conversationId = isDraft ? "fixture-draft" : "fixture-existing";
  return (
    <ProjectsContext value={client.projectsPresentation}>
      <main className="flex min-h-dvh flex-col gap-6 bg-pane p-4 text-foreground">
        <p className="text-sm text-muted">
          Synthetic projects with real owner commands and in-memory persistence. No live data.
        </p>
        <div className="flex flex-wrap gap-2">
          <Button size="sm" variant="secondary" onPress={() => setThemePreference("light")}>
            Light
          </Button>
          <Button size="sm" variant="secondary" onPress={() => setThemePreference("dark")}>
            Dark
          </Button>
          <Button size="sm" variant="secondary" onPress={() => setDraft(!isDraft)}>
            {isDraft ? "Use existing conversation" : "Use draft"}
          </Button>
          <Button
            size="sm"
            variant="secondary"
            isDisabled={many}
            onPress={() => {
              setMany(true);
              Promise.all(
                Array.from({ length: 36 }, (_, index) =>
                  client.projects.create({ name: `Synthetic project ${index + 1}` }),
                ),
              ).catch(client.projects.fail);
            }}
          >
            Add many projects
          </Button>
        </div>
        <output aria-label="Project selection" className="text-sm wrap-anywhere">
          {JSON.stringify({
            isDraft,
            draft: state.draftLinks["fixture-draft"] ?? null,
            saved: state.links["fixture-existing"] ?? null,
            dialogOpen: state.dialog !== null,
            error: state.error,
          })}
        </output>
        <div className="mx-auto mt-auto w-full max-w-2xl">
          <ComposerPanel>
            <ComposerPanel.Context>
              <ProjectSelector conversationId={conversationId} isDraft={isDraft} />
            </ComposerPanel.Context>
            <ComposerPanel.Body>
              <div className="fixture-composer-body" />
            </ComposerPanel.Body>
          </ComposerPanel>
        </div>
      </main>
      <ProjectDialog />
    </ProjectsContext>
  );
}

const element = document.getElementById("root");
if (element === null) {
  throw new Error("The fixture root is missing.");
}
const root = createRoot(element);
root.render(<Fixture />);
import.meta.hot?.dispose(() => {
  root.unmount();
  client.close().catch(console.error);
});
