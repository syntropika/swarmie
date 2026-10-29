import type { Preview } from "@storybook/react-vite";
import { useLayoutEffect } from "react";
import type { ReactNode } from "react";

import "./preview.css";

function CatalogTheme({ children, theme }: { children: ReactNode; theme: "light" | "dark" }) {
  useLayoutEffect(() => {
    document.documentElement.dataset.theme = theme;
  }, [theme]);
  return (
    <div
      data-theme={theme}
      className="min-h-screen bg-background p-4 font-sans text-foreground antialiased"
    >
      {children}
    </div>
  );
}

const preview: Preview = {
  tags: ["autodocs"],
  decorators: [
    (Story, context) => (
      <CatalogTheme theme={context.globals.theme === "dark" ? "dark" : "light"}>
        <Story />
      </CatalogTheme>
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
    layout: "fullscreen",
    a11y: { test: "error" },
    options: {
      storySort: {
        method: "alphabetical",
        order: [
          "Foundations",
          [
            "Overview",
            "Brand",
            "Responsive Design",
            "Typography",
            "Colors",
            "Spacing",
            "Shadows",
            "Motion",
            "Icons",
          ],
          "Atoms",
          "Molecules",
        ],
      },
    },
  },
  initialGlobals: { theme: "light" },
};

export default preview;
