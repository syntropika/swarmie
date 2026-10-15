import { Surface, SwarmieMark } from "@swarmie/ui-web";
import type { ReactNode } from "react";

import { LoginIcon } from "~/src/identity/shared/components/icon";
import { useLocale } from "~/src/platform/i18n/provider";
import { PageFooter } from "~/src/platform/shell/components/page-footer";

function AuthHeader({ homeHref }: { homeHref: string }) {
  const { t } = useLocale();
  return (
    <header className="flex min-h-18 items-center justify-between gap-3 rounded-marketing-shell bg-surface px-5 py-3 shadow-raised sm:px-8">
      <a
        className="inline-flex min-h-control w-max items-center gap-2 rounded-pill text-brand leading-none font-semibold tracking-brand text-ink no-underline focus-visible:outline-2 focus-visible:outline-offset-4 focus-visible:outline-primary sm:text-brand-large pointer-coarse:min-h-control-touch"
        href={homeHref}
        aria-label={t("common.home")}
      >
        <SwarmieMark className="size-8" />
        swarmie
      </a>
      <a
        className="inline-flex min-h-control items-center gap-2 rounded-full px-3 text-ui font-medium text-text-muted transition-colors duration-150 hover:bg-surface-hover hover:text-primary-hover focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-primary motion-reduce:transition-none pointer-coarse:min-h-control-touch"
        href={homeHref}
        aria-label={t("common.backHome")}
      >
        <LoginIcon name="back" className="max-[760px]:size-6" />
        <span className="max-[760px]:hidden">{t("common.backHome")}</span>
      </a>
    </header>
  );
}

/** The shared identity frame keeps each account workflow responsible for its own content. */
export function AuthPage({ homeHref, children }: { homeHref: string; children: ReactNode }) {
  return (
    <main className="@container mx-auto flex min-h-svh w-full max-w-[1344px] flex-col px-6 pt-4 sm:px-10 lg:px-12">
      <AuthHeader homeHref={homeHref} />
      <div className="grid flex-1 items-center gap-8 py-10 @3xl:grid-cols-2 @3xl:gap-10 @3xl:py-14 @5xl:gap-20">
        {children}
      </div>
      <PageFooter />
    </main>
  );
}

export function AuthPanel({
  label,
  illustration,
  children,
}: {
  label: string;
  illustration: ReactNode;
  children: ReactNode;
}) {
  return (
    <section className="min-w-0" aria-label={label}>
      <Surface elevation="raised" inset="spacious">
        {children}
        <div className="mx-auto mt-6 hidden w-full max-w-60 @3xl:block">{illustration}</div>
      </Surface>
    </section>
  );
}

export function AuthIntroduction({ title, description }: { title: string; description: string }) {
  return (
    <div className="mb-7">
      <h2 className="text-title font-medium text-balance">{title}</h2>
      <p className="mt-3 text-ui leading-relaxed text-text-muted">{description}</p>
    </div>
  );
}
