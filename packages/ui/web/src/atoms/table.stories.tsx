import type { Meta, StoryObj } from "@storybook/react-vite";

import { officialExampleSource } from "../../.storybook/example-sources";
import { OfficialExample as HeroUIDocumentationExample } from "../../.storybook/examples/table";
import { HeroUIPreview } from "../../.storybook/heroui-preview";
import { Table } from "./table";

const projects = [
  { id: "website", name: "Website", status: "In review", owner: "Alex" },
  { id: "desktop", name: "Desktop client", status: "In progress", owner: "Sam" },
  { id: "shared", name: "Shared components", status: "Ready", owner: "Jordan" },
];

const meta = {
  title: "Atoms/Table",
  component: Table,
  decorators: [
    (Story, context) => (
      <HeroUIPreview theme={context.globals.theme === "dark" ? "dark" : "light"}>
        <Story />
      </HeroUIPreview>
    ),
  ],
} satisfies Meta<typeof Table>;
export default meta;
type Story = StoryObj<typeof meta>;

function ProjectTable({
  secondary = false,
  empty = false,
}: {
  secondary?: boolean;
  empty?: boolean;
}) {
  return (
    <Table variant={secondary ? "secondary" : "primary"}>
      <Table.ScrollContainer>
        <Table.Content aria-label="Workspace projects">
          <Table.Header>
            <Table.Column isRowHeader>Project</Table.Column>
            <Table.Column>Status</Table.Column>
            <Table.Column>Owner</Table.Column>
          </Table.Header>
          <Table.Body
            items={empty ? [] : projects}
            renderEmptyState={() => "No projects match this filter."}
          >
            {(project) => (
              <Table.Row id={project.id}>
                <Table.Cell>{project.name}</Table.Cell>
                <Table.Cell>{project.status}</Table.Cell>
                <Table.Cell>{project.owner}</Table.Cell>
              </Table.Row>
            )}
          </Table.Body>
        </Table.Content>
      </Table.ScrollContainer>
      <Table.Footer>{empty ? "No projects" : "3 projects"}</Table.Footer>
    </Table>
  );
}

export const Default: Story = { render: () => <ProjectTable /> };
export const Secondary: Story = { render: () => <ProjectTable secondary /> };
export const Empty: Story = { render: () => <ProjectTable empty /> };

export const OfficialExample: Story = {
  parameters: { docs: { source: { code: officialExampleSource("table"), language: "tsx" } } },
  name: "Official example",
  render: () => <HeroUIDocumentationExample />,
};
