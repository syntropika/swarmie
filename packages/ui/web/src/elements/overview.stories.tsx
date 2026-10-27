import type { Meta, StoryObj } from "@storybook/react-vite";

import catalog from "../../../../../scripts/ui-elements/catalog.json";
import { Link } from "../atoms/link";
import { Reference, Section } from "../foundations/reference";

function storyId(section: string, title: string, key: string) {
  const category = section === "AUI connected" ? "Runtime-neutral compositions" : section;
  const slug = `Assistant Elements/${category}/${title}`
    .toLowerCase()
    .replaceAll(/[^a-z0-9]+/gu, "-")
    .replaceAll(/^-|-$/gu, "");
  return `${slug}--${key}`;
}
function Overview() {
  return (
    <Reference
      title="Assistant Elements"
      intro="The official Elements catalog, ported onto shared HeroUI controls. Browse all 151 entries and their presentation states."
    >
      <Section title="Use with host-owned state">
        <p className="text-ui">
          Import components from @swarmie/ui-web/elements. They accept data and callbacks. They do
          not import assistant-ui runtime providers, external stores or agent execution.
          ConversationThread receives the transcript and actions from its consumer.
        </p>
        <p className="text-ui text-text-muted">
          Upstream .aui bindings are replaced with runtime-neutral presentations in the compositions
          section. These examples use temporary local fixture state; no model, server or Tembra node
          is connected.
        </p>
      </Section>
      {catalog.map((section) => (
        <Section
          key={section.label}
          title={section.label === "AUI connected" ? "Runtime-neutral compositions" : section.label}
          description={section.description}
        >
          <ul className="grid gap-3 @min-[36rem]:grid-cols-2">
            {section.elements.map((entry) => (
              <li
                key={entry.slug}
                className="grid min-w-0 gap-2 rounded-panel border border-border p-4"
              >
                <Link
                  target="_top"
                  href={`?path=/story/${storyId(section.label, entry.title, ("variants" in entry ? entry.variants?.[0]?.key : undefined) ?? "default")}`}
                >
                  {entry.title}
                </Link>
                <p className="text-ui text-text-muted">{entry.description}</p>
              </li>
            ))}
          </ul>
        </Section>
      ))}
      <Section title="Source and adaptation">
        <p className="text-ui">
          Source revision: d8124e7b427021d977abf9aabc9a057705a81644. Original source and its MIT
          license are preserved with integrity hashes. Buttons, fields, overlays and disclosures use
          shared HeroUI components. Diagrams, media and other specialized views retain their
          dedicated renderers.
        </p>
        <Link href="https://www.assistant-ui.com/elements">Official Elements catalog</Link>
      </Section>
    </Reference>
  );
}
const meta = {
  id: "assistant-ui-overview",
  title: "Assistant Elements/Overview",
  parameters: { layout: "fullscreen" },
} satisfies Meta;
export default meta;
type Story = StoryObj<typeof meta>;
export const Catalog: Story = { render: () => <Overview /> };
