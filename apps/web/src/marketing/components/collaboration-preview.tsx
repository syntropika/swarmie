import { Avatar, Badge, Icon } from "@swarmie/ui-web";

import { useLocale } from "~/src/platform/i18n/provider";

const contextSources = ["demo.sourceOne", "demo.sourceTwo", "demo.sourceThree"] as const;

/** A composed product scene illustrating an onboarding Agent, not a live interaction. */
export function CollaborationPreview() {
  const { t, formatTime } = useLocale();

  return (
    <div
      data-motion-scene="onboarding"
      className="relative isolate overflow-hidden rounded-4xl bg-primary-soft/65 p-3 sm:p-6 lg:p-8"
    >
      <div
        aria-hidden="true"
        className="pointer-events-none absolute -top-24 -right-16 size-64 rounded-full bg-lilac/65"
      />
      <div
        aria-hidden="true"
        className="pointer-events-none absolute -bottom-28 -left-12 size-64 rounded-full bg-mint/65"
      />

      <div className="relative z-10 rounded-3xl bg-surface p-5 shadow-raised sm:p-7">
        <div className="flex items-center gap-3 pb-5">
          <span aria-hidden="true" className="flex items-center gap-1">
            <Avatar kind="person" initials="MC" label="Maya Chen" className="size-8" />
            <Avatar kind="agent" initials="SW" label="Swarmie" className="size-8" />
          </span>
          <span className="min-w-0 text-ui font-medium text-ink">{t("demo.topic")}</span>
        </div>

        <div
          data-motion-part="onboarding-question"
          className="mt-6 flex flex-row-reverse gap-3 sm:gap-4"
        >
          <Avatar kind="person" initials="MC" label="Maya Chen" className="size-9 shrink-0" />
          <div className="min-w-0">
            <p className="flex flex-wrap items-baseline justify-end gap-x-2 text-ui font-medium text-ink">
              Maya Chen
              <time dateTime="09:41" className="text-xs font-normal text-text-muted">
                {formatTime(9, 41)}
              </time>
            </p>
            <p className="mt-2 max-w-[40ch] rounded-panel bg-surface-muted px-4 py-3 text-ui leading-relaxed text-pretty text-ink sm:text-body">
              {t("demo.personMessage")}
            </p>
          </div>
        </div>

        <div className="mt-6 py-5 sm:ml-12">
          <p
            data-motion-part="onboarding-context-label"
            className="text-xs font-medium text-text-muted"
          >
            {t("demo.contextLabel")}
          </p>
          <ul data-motion-part="onboarding-sources" className="mt-3 flex flex-wrap gap-2">
            {contextSources.map((source) => (
              <li
                key={source}
                className="inline-flex min-h-9 items-center gap-2 rounded-full bg-primary-soft px-3 py-1.5 text-xs font-medium text-ink"
              >
                <span aria-hidden="true" className="size-1.5 rounded-full bg-primary" />
                {t(source)}
              </li>
            ))}
          </ul>
        </div>

        <div data-motion-part="onboarding-answer" className="mt-6 flex gap-3 sm:gap-4">
          <Avatar kind="agent" initials="SW" label="Swarmie" className="size-9 shrink-0" />
          <div className="min-w-0">
            <p className="flex flex-wrap items-center gap-x-2 gap-y-1 text-ui font-medium text-ink">
              Swarmie
              <Badge tone="agent">{t("demo.agent")}</Badge>
              <time dateTime="09:42" className="text-xs font-normal text-text-muted">
                {formatTime(9, 42)}
              </time>
            </p>
            <p className="mt-2 max-w-[48ch] text-ui leading-relaxed text-pretty text-ink sm:text-body">
              {t("demo.agentMessage")}
            </p>
          </div>
        </div>
      </div>

      <div
        data-motion-part="onboarding-suggestion"
        className="relative z-20 mt-3 rounded-3xl bg-surface p-5 shadow-raised sm:mt-[-24px] sm:ml-auto sm:w-[min(85%,440px)] sm:p-6 lg:mr-2"
      >
        <div className="flex items-center gap-3">
          <span
            aria-hidden="true"
            className="flex size-9 shrink-0 items-center justify-center rounded-pill bg-mint text-ink"
          >
            <Icon name="check" className="size-5" />
          </span>
          <p className="text-xs font-semibold text-primary">
            {t("demo.suggestionLabel", { agent: "Swarmie" })}
          </p>
        </div>
        <p className="mt-4 text-body leading-snug font-semibold text-ink sm:text-lg">
          {t("demo.suggestionText")}
        </p>
        <p className="mt-3 text-ui leading-relaxed text-text-muted">{t("demo.decisionNote")}</p>
      </div>
    </div>
  );
}
