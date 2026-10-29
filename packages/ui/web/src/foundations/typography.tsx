import { HeroUIPreview } from "../../.storybook/heroui-preview";
import { Table } from "../atoms/table";
import { Reference } from "./reference";

const scales = [
  ["Caption", "text-xs", "Metadata and supporting labels"],
  ["Interface", "text-sm", "Vendor controls and compact content"],
  ["Body", "text-base", "Reading and editable text"],
  ["Emphasis", "text-lg", "Prominent content"],
  ["Section", "text-xl", "Section headings"],
  ["Title", "text-2xl", "Page headings"],
  ["Display", "text-3xl", "Short prominent headings"],
] as const;
const weights = [
  ["Body copy", "400", "font-normal"],
  ["Actions and labels", "500", "font-medium"],
  ["Headlines", "600", "font-semibold"],
  ["Strong emphasis", "700", "font-bold"],
] as const;

export function TypographyReference({ theme }: { theme: "light" | "dark" }) {
  return (
    <Reference
      title="Typography"
      intro="Google Sans is the shared typeface for headings, controls, reading across web and desktop. Code uses the operating system’s monospace stack; canonical logo artwork keeps its original lettering."
    >
      <HeroUIPreview theme={theme}>
        <section className="grid gap-4">
          <h2 className="text-xl font-semibold">One typeface, clear roles</h2>
          <p className="text-3xl font-semibold">A place for the next idea.</p>
          <p className="text-base">Review the proposal, add a note, and keep working.</p>
          <p className="font-mono text-sm">const nextStep = &quot;Review changes&quot;;</p>
        </section>
        <section className="grid gap-4">
          <h2 className="text-xl font-semibold">Type scale</h2>
          <Table>
            <Table.ScrollContainer>
              <Table.Content aria-label="Swarmie type scale">
                <Table.Header>
                  <Table.Column isRowHeader>Role</Table.Column>
                  <Table.Column>Utility and use</Table.Column>
                  <Table.Column>Sample</Table.Column>
                </Table.Header>
                <Table.Body>
                  {scales.map(([name, className, usage]) => (
                    <Table.Row key={name} id={name}>
                      <Table.Cell>{name}</Table.Cell>
                      <Table.Cell>
                        <code>{className}</code>
                        <p className="mt-1 text-sm text-muted">{usage}</p>
                      </Table.Cell>
                      <Table.Cell>
                        <span className={className}>Shared progress</span>
                      </Table.Cell>
                    </Table.Row>
                  ))}
                </Table.Body>
              </Table.Content>
            </Table.ScrollContainer>
            <Table.Footer>The same scale is used by web and desktop.</Table.Footer>
          </Table>
        </section>
        <section className="grid gap-4">
          <h2 className="text-xl font-semibold">Weights</h2>
          <Table>
            <Table.ScrollContainer>
              <Table.Content aria-label="Typography weights">
                <Table.Header>
                  <Table.Column isRowHeader>Use</Table.Column>
                  <Table.Column>Weight</Table.Column>
                  <Table.Column>Sample</Table.Column>
                </Table.Header>
                <Table.Body>
                  {weights.map(([name, weight, className]) => (
                    <Table.Row key={weight} id={weight}>
                      <Table.Cell>{name}</Table.Cell>
                      <Table.Cell>{weight}</Table.Cell>
                      <Table.Cell className={className}>Review the proposal</Table.Cell>
                    </Table.Row>
                  ))}
                </Table.Body>
              </Table.Content>
            </Table.ScrollContainer>
          </Table>
        </section>
        <section className="grid gap-4">
          <h2 className="text-xl font-semibold">Line height and density</h2>
          <p className="max-w-3xl text-base">
            The vendor controls supply their text size and line height. Reading content uses a
            bounded width. Google Sans is shared across the catalog, web and desktop.
          </p>
          <p className="text-sm">
            Use the public size and variant props when a control offers them. Do not add a second
            generic control scale in a consumer.
          </p>
        </section>
      </HeroUIPreview>
    </Reference>
  );
}
