import type { Meta, StoryObj } from "@storybook/react-vite";

import { officialExampleSource } from "../../.storybook/example-sources";
import { OfficialExample as HeroUIDocumentationExample } from "../../.storybook/examples/alert";
import { Icon } from "../atoms/icon";
import { Link } from "../atoms/link";
import { Alert } from "./alert";

const meta = {
  title: "Atoms/Alert",
  component: Alert,
  args: {
    status: "accent",
    role: "status",
    "aria-atomic": true,
    children: (
      <>
        <Alert.Indicator>
          <Icon name="circle-attention" />
        </Alert.Indicator>
        <Alert.Content>
          <Alert.Title>Your changes are saved.</Alert.Title>
          <Alert.Description>Everyone with access will see the latest version.</Alert.Description>
        </Alert.Content>
      </>
    ),
  },
  decorators: [
    (Story) => (
      <div className="max-w-md">
        <Story />
      </div>
    ),
  ],
  parameters: {
    docs: {
      description: {
        component:
          "Compose optional icon, title, description, and ordinary actions as children. HeroUI status controls presentation. Set role and aria-atomic for feedback that needs a live region. The consumer owns content and when a notice appears.",
      },
    },
  },
} satisfies Meta<typeof Alert>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Information: Story = {};

export const Success: Story = { args: { status: "success" } };

export const ErrorWithAction: Story = {
  args: {
    status: "danger",
    role: "alert",
    children: (
      <>
        <Alert.Indicator>
          <Icon name="circle-attention" />
        </Alert.Indicator>
        <Alert.Content>
          <Alert.Title>We couldn’t load your changes.</Alert.Title>
          <Alert.Description>Check your connection and open the page again.</Alert.Description>
          <Link className="mt-3" href="#retry-example">
            Open the page
          </Link>
        </Alert.Content>
      </>
    ),
  },
};

export const WithoutIcon: Story = {
  args: {
    children: (
      <Alert.Content>
        <Alert.Title>All caught up.</Alert.Title>
        <Alert.Description>There are no new updates.</Alert.Description>
      </Alert.Content>
    ),
  },
};

export const OfficialExample: Story = {
  parameters: { docs: { source: { code: officialExampleSource("alert"), language: "tsx" } } },
  name: "Official example",
  render: () => <HeroUIDocumentationExample />,
};
