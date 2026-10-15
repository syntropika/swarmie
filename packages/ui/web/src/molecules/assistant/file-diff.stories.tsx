import type { Meta, StoryObj } from "@storybook/react-vite";

import { ExampleFrame } from "./examples/fixtures";
import { FileDiff } from "./file-diff";

const meta = {
  title: "Assistant/Content & data/FileDiff",
  component: FileDiff,
  parameters: { layout: "fullscreen" },
} satisfies Meta<typeof FileDiff>;
export default meta;
type Story = StoryObj;

export const Default: Story = {
  render: () => (
    <ExampleFrame>
      <FileDiff
        label="Proposed source changes"
        describeRow={(row) =>
          `${row.kind}, old line ${row.oldLine ?? "none"}, new line ${row.newLine ?? "none"}`
        }
        file="src/session.ts"
        rows={[
          {
            id: "open",
            kind: "context",
            oldLine: 12,
            newLine: 12,
            text: "export function sessionTitle(session) {",
          },
          {
            id: "old",
            kind: "removed",
            oldLine: 13,
            newLine: null,
            text: "  return session.title || 'Untitled';",
          },
          {
            id: "new",
            kind: "added",
            oldLine: null,
            newLine: 13,
            text: "  return session.title.trim() || 'New session';",
          },
          { id: "close", kind: "context", oldLine: 14, newLine: 14, text: "}" },
        ]}
      />
    </ExampleFrame>
  ),
};
