import { Avatar, Icon } from "@swarmie/ui-web";

import { useLocale } from "~/src/platform/i18n/provider";

/** An illustrative exchange, with a person making the decision after an Agent's suggestion. */
export function SharedSurfacesIllustration() {
  const { t } = useLocale();

  return (
    <div
      data-motion-scene="hero"
      className="relative isolate w-full overflow-hidden rounded-marketing-shell bg-primary-soft p-3 sm:p-6"
    >
      <div aria-hidden="true" className="absolute -top-12 -right-12 size-48 rounded-full bg-mint" />
      <div
        aria-hidden="true"
        className="absolute -bottom-16 -left-12 size-40 rounded-full bg-lilac"
      />

      <div className="relative overflow-hidden rounded-marketing-card border border-transparent bg-surface shadow-raised">
        <div className="flex items-center justify-between gap-4 px-5 py-4 sm:px-6">
          <p className="min-w-0 text-ui leading-ui font-medium text-ink">{t("hero.sceneTitle")}</p>
          <span aria-hidden="true" className="flex shrink-0 gap-1.5">
            <span className="size-1.5 rounded-full bg-border" />
            <span className="size-1.5 rounded-full bg-border" />
            <span className="size-1.5 rounded-full bg-primary" />
          </span>
        </div>

        <div className="grid gap-5 px-5 py-6 sm:px-6 sm:py-7">
          <div data-motion-part="hero-question" className="flex items-start gap-3">
            <Avatar className="size-9 shrink-0" aria-label="Maya Chen" color="default">
              <Avatar.Fallback>MC</Avatar.Fallback>
            </Avatar>
            <div className="min-w-0 flex-1">
              <p className="flex flex-wrap items-baseline gap-x-2 text-ui leading-5">
                <span className="font-semibold text-ink">Maya Chen</span>
                <span className="text-xs text-text-muted">{t("hero.scenePerson")}</span>
              </p>
              <p className="mt-2 text-body leading-6 text-ink">{t("hero.sceneQuestion")}</p>
            </div>
          </div>

          <div className="ml-12 flex items-center gap-2.5">
            <span
              aria-hidden="true"
              data-motion-part="hero-bridge"
              className="h-px w-6 origin-left bg-primary/40"
            />
            <span
              data-motion-part="hero-context"
              className="rounded-full bg-primary-soft px-3 py-1.5 text-xs font-medium text-primary"
            >
              {t("hero.sceneContext")}
            </span>
          </div>

          <div data-motion-part="hero-answer" className="flex items-start gap-3">
            <Avatar className="size-9 shrink-0" aria-label="Swarmie" color="accent">
              <Avatar.Fallback>SW</Avatar.Fallback>
            </Avatar>
            <div className="min-w-0 flex-1">
              <p className="flex flex-wrap items-baseline gap-x-2 text-ui leading-5">
                <span className="font-semibold text-ink">Swarmie</span>
                <span className="text-xs text-text-muted">{t("hero.sceneAgent")}</span>
              </p>
              <p className="mt-2 max-w-[40ch] text-body leading-6 text-ink">
                {t("hero.sceneAnswer")}
              </p>
            </div>
          </div>
        </div>

        <div
          data-motion-part="hero-decision"
          className="flex items-start gap-3 bg-surface-muted px-5 py-4 sm:px-6"
        >
          <span
            aria-hidden="true"
            className="mt-0.5 inline-flex size-5 shrink-0 items-center justify-center rounded-full bg-primary text-on-primary"
          >
            <Icon name="check" className="size-3.5" />
          </span>
          <p className="text-ui leading-5 font-medium text-ink">{t("hero.sceneDecision")}</p>
        </div>
      </div>
    </div>
  );
}
