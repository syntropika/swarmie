import type { Meta, StoryObj } from "@storybook/react-vite";
import { useState } from "react";

import { ChromeButton } from "../atoms/chrome-button";
import { Icon } from "../atoms/icon";
import { NavigationGroup } from "./navigation-group";

function Example() {
  const [expanded, setExpanded] = useState(true);
  return (
    <div className="w-64">
      <NavigationGroup
        title="Projects"
        expanded={expanded}
        onExpandedChange={setExpanded}
        actions={
          <>
            <ChromeButton aria-label="Project options">
              <Icon name="dots" />
            </ChromeButton>
            <ChromeButton aria-label="Create project">
              <Icon name="plus" />
            </ChromeButton>
          </>
        }
      >
        <ChromeButton size="navigation">Design system</ChromeButton>
        <ChromeButton size="navigation">Application</ChromeButton>
      </NavigationGroup>
    </div>
  );
}
const meta = { title: "Molecules/Navigation Group", component: Example } satisfies Meta<
  typeof Example
>;
export default meta;
export const Default: StoryObj<typeof meta> = {};
