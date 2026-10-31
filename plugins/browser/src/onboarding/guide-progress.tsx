import { cn, Icon } from "@swarmie/ui-web";

export type GuideStep = "provider" | "configuration" | "ready";

const steps = [
  { id: "provider", title: "Choose a provider" },
  { id: "configuration", title: "Configure provider" },
  { id: "ready", title: "Ready to work" },
] as const;

export function GuideProgress({ step }: { readonly step: GuideStep }) {
  const active = steps.findIndex((entry) => entry.id === step);
  return (
    <ol
      className="relative grid grid-cols-3 gap-3 before:pointer-events-none before:absolute before:top-3.5 before:bottom-3.5 before:left-5.5 before:hidden before:border-l before:border-dashed before:border-border @min-[720px]/onboarding:grid-cols-1 @min-[720px]/onboarding:gap-8 @min-[720px]/onboarding:before:block"
      aria-label="Setup progress"
    >
      {steps.map((entry, index) => {
        const done = index < active;
        const current = index === active;
        let tone = done ? "text-foreground" : "text-muted";
        if (current) {
          tone = "text-brand-accent-text";
        }
        return (
          <li
            key={entry.id}
            className="flex flex-col items-start gap-2 @min-[720px]/onboarding:flex-row @min-[720px]/onboarding:items-center @min-[720px]/onboarding:gap-3"
            aria-current={current ? "step" : undefined}
          >
            <span className="relative z-10 grid w-11 shrink-0 place-items-center">
              <span
                className={cn(
                  "grid size-7 shrink-0 place-items-center rounded-full text-xs font-medium",
                  done || current ? "bg-accent text-on-primary" : "bg-default text-muted",
                )}
              >
                {done ? <Icon name="check" /> : index + 1}
              </span>
            </span>
            <span className={tone}>{entry.title}</span>
          </li>
        );
      })}
    </ol>
  );
}
