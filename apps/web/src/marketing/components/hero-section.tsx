import { SharedSurfacesIllustration } from "~/src/marketing/components/shared-surfaces-illustration";
import { NewsletterSignup } from "~/src/newsletter/newsletter-signup";
import { useLocale } from "~/src/platform/i18n/provider";

export function HeroSection() {
  const { t } = useLocale();
  return (
    <section
      aria-labelledby="hero-title"
      className="mx-auto grid w-[min(1248px,calc(100%_-_48px))] items-center gap-12 py-14 md:w-[min(1248px,calc(100%_-_96px))] md:py-20 lg:min-h-[680px] lg:grid-cols-[minmax(0,1.05fr)_minmax(0,0.95fr)] lg:gap-12"
    >
      <div className="min-w-0">
        <h1
          id="hero-title"
          className="max-w-[720px] text-marketing-display font-medium text-balance text-ink"
        >
          {t("hero.title")} <span className="block text-ink">{t("hero.titleAccent")}</span>
        </h1>
        <p className="mt-6 max-w-[48ch] text-lg leading-relaxed text-pretty text-text-muted md:text-xl">
          {t("hero.description")}
        </p>
        <div id="signup" className="scroll-mt-28">
          <NewsletterSignup />
        </div>
      </div>
      <div className="mx-auto w-full max-w-[600px]">
        <SharedSurfacesIllustration />
      </div>
    </section>
  );
}
