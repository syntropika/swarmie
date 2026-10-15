import { CollaborationPreview } from "~/src/marketing/components/collaboration-preview";
import { useLocale } from "~/src/platform/i18n/provider";

export function CollaborationSection() {
  const { t } = useLocale();

  return (
    <section
      id="collaboration"
      aria-labelledby="collaboration-title"
      className="mx-auto grid w-[min(1248px,calc(100%_-_48px))] scroll-mt-24 gap-10 py-20 md:w-[min(1248px,calc(100%_-_96px))] lg:grid-cols-[minmax(0,0.75fr)_minmax(0,1.25fr)] lg:items-center lg:gap-16 lg:py-28"
    >
      <div>
        <h2
          id="collaboration-title"
          aria-label={`${t("collaboration.title")} ${t("collaboration.titleSecond")}`}
          className="max-w-[460px] text-marketing-section font-medium text-balance"
        >
          {t("collaboration.title")}{" "}
          <span className="lg:block">{t("collaboration.titleSecond")}</span>
        </h2>
        <p className="mt-6 max-w-[44ch] text-body leading-relaxed text-pretty text-text-muted sm:text-lg">
          {t("collaboration.description")}
        </p>
      </div>
      <CollaborationPreview />
    </section>
  );
}
