import { Link } from "@swarmie/ui-web";

import { CapabilitiesSection } from "~/src/marketing/components/capabilities-section";
import { CollaborationSection } from "~/src/marketing/components/collaboration-section";
import { HeroSection } from "~/src/marketing/components/hero-section";
import { ProblemSection } from "~/src/marketing/components/problem-section";
import { ScenarioSection } from "~/src/marketing/components/scenario-section";
import { SiteFooter } from "~/src/marketing/components/site-footer";
import { SiteHeader } from "~/src/marketing/components/site-header";
import { useMarketingMotion } from "~/src/marketing/marketing-motion";
import { NewsletterSignupProvider } from "~/src/newsletter/newsletter-signup";
import { useLocale } from "~/src/platform/i18n/provider";

/** Compose the public product introduction from independently readable sections. */
export function HomePage() {
  const { t } = useLocale();
  useMarketingMotion();
  return (
    <NewsletterSignupProvider>
      <Link href="#main" className="sr-only fixed top-4 left-4 z-50 focus:not-sr-only">
        {t("common.skip")}
      </Link>
      <SiteHeader />
      <main id="main" tabIndex={-1}>
        <HeroSection />
        <ProblemSection />
        <CollaborationSection />
        <CapabilitiesSection />
        <ScenarioSection />
      </main>
      <SiteFooter />
    </NewsletterSignupProvider>
  );
}
