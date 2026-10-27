import { Icon } from "@swarmie/ui-web";

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
      className="relative grid gap-8 text-sm before:absolute before:top-8 before:bottom-8 before:left-4 before:border-l before:border-dashed before:border-border"
      aria-label="Setup progress"
    >
      {steps.map((entry, index) => {
        const done = index < active;
        const current = index === active;
        let tone = "text-muted";
        if (done) {
          tone = "text-foreground";
        }
        if (current) {
          tone = "text-brand-accent";
        }
        return (
          <li
            key={entry.id}
            className={`flex items-center gap-3 ${tone}`}
            aria-current={current ? "step" : undefined}
          >
            <span
              className={`relative flex size-8 shrink-0 items-center justify-center rounded-full border ${done || current ? "border-brand-accent bg-brand-accent text-brand-accent-foreground" : "border-border"}`}
            >
              {done ? <Icon name="check" className="size-3.5" /> : index + 1}
            </span>
            {entry.title}
          </li>
        );
      })}
    </ol>
  );
}
