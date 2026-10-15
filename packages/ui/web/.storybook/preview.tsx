import type { Preview } from "@storybook/react-vite";

import "../src/styles.css";

const preview: Preview = {
  tags: ["autodocs"],
  decorators: [
    (Story, context) => (
      <div
        data-theme={context.globals.theme === "dark" ? "dark" : "light"}
        className="min-h-screen bg-surface p-4 font-sans leading-[1.6] text-ink antialiased selection:bg-primary-soft selection:text-ink [&_:focus-visible]:outline-2 [&_:focus-visible]:outline-offset-2 [&_:focus-visible]:outline-primary [&_a]:underline-offset-3"
      >
        <Story />
      </div>
    ),
  ],
  globalTypes: {
    theme: {
      description: "Swarmie theme",
      toolbar: {
        icon: "mirror",
        items: [
          { value: "light", title: "Light" },
          { value: "dark", title: "Dark" },
        ],
        dynamicTitle: true,
      },
    },
  },
  parameters: {
    layout: "padded",
    backgrounds: {
      options: {
        surface: { name: "Surface", value: "#FCFAF7" },
        canvas: { name: "Canvas", value: "#F4F1EC" },
      },
    },
    a11y: { test: "error" },
    options: {
      storySort: {
        method: "alphabetical",
        order: [
          "Foundations",
          "Atoms",
          "Molecules",
          "Assistant",
          [
            "Conversations",
            "Composer",
            "Messages",
            "Loading",
            "Reasoning",
            "Tools",
            "Approvals & forms",
            "Sources & attachments",
            "Content & data",
            "Agents & tasks",
            "Generative UI",
            "Voice",
            "Observability",
            "Primitives",
          ],
        ],
      },
    },
  },
  initialGlobals: { theme: "light", backgrounds: { value: "surface" } },
};

export default preview;
