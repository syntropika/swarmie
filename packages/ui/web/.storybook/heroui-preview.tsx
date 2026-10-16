import type { ReactNode } from "react";

/** A themed specimen uses the same shared vendor stylesheet as application consumers. */
export function HeroUIPreview({
  children,
  theme,
}: {
  children: ReactNode;
  theme: "light" | "dark";
}) {
  return (
    <div
      data-theme={theme}
      data-heroui-preview
      data-testid="hero-preview-host"
      data-heroui-palette="swarmie"
      className="@container grid min-w-0 gap-8 rounded-2xl bg-background p-6 font-sans text-foreground"
    >
      {children}
    </div>
  );
}
