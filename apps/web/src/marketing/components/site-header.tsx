import { Button, Icon, SwarmieMark } from "@swarmie/ui-web";
import { useEffect, useRef, useState } from "react";

import { ArrowRight } from "~/src/marketing/components/icons";
import { useLocale } from "~/src/platform/i18n/provider";

const sectionLinks = [
  { href: "#collaboration", label: "common.collaboration" },
  { href: "#factory", label: "common.factory" },
  { href: "#capabilities", label: "common.capabilities" },
] as const;

export function SiteHeader() {
  const { t } = useLocale();
  const [menuOpen, setMenuOpen] = useState(false);
  const headerRef = useRef<HTMLElement>(null);
  const menuButtonRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    if (!menuOpen) {
      return undefined;
    }

    function closeOnEscape(event: KeyboardEvent) {
      if (event.key === "Escape") {
        setMenuOpen(false);
        menuButtonRef.current?.focus();
      }
    }

    function closeOutside(event: PointerEvent) {
      if (event.target instanceof Node && headerRef.current?.contains(event.target) !== true) {
        setMenuOpen(false);
      }
    }

    document.addEventListener("keydown", closeOnEscape);
    document.addEventListener("pointerdown", closeOutside);

    return () => {
      document.removeEventListener("keydown", closeOnEscape);
      document.removeEventListener("pointerdown", closeOutside);
    };
  }, [menuOpen]);

  return (
    <header
      id="top"
      ref={headerRef}
      className="sticky top-3 z-40 mx-auto mt-3 w-[min(1248px,calc(100%_-_48px))] rounded-marketing-shell bg-surface shadow-raised md:w-[min(1248px,calc(100%_-_96px))]"
    >
      <div className="flex min-h-16 items-center justify-between gap-4 px-4 sm:px-6 lg:px-8">
        <a
          href="#top"
          aria-label={t("common.home")}
          onClick={() => {
            setMenuOpen(false);
          }}
          className="inline-flex min-h-control shrink-0 items-center gap-2 rounded-pill text-brand leading-none font-semibold tracking-brand no-underline focus-visible:outline-2 focus-visible:outline-offset-4 focus-visible:outline-primary sm:gap-2.5 sm:text-brand-large pointer-coarse:min-h-control-touch"
        >
          <SwarmieMark className="size-8 sm:size-10" />
          swarmie
        </a>

        <nav aria-label={t("common.navigation")} className="hidden items-center gap-1 xl:flex">
          {sectionLinks.map(({ href, label }) => (
            <a
              key={href}
              href={href}
              className="inline-flex min-h-control items-center rounded-full px-4 text-ui font-medium text-text-muted no-underline transition-colors duration-150 hover:bg-surface-muted hover:text-ink focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-primary motion-reduce:transition-none pointer-coarse:min-h-control-touch"
            >
              {t(label)}
            </a>
          ))}
          <div className="group relative ml-3">
            <Button
              variant="brand"
              size="brand"
              aria-disabled="true"
              aria-describedby="public-access-notice"
              className="cursor-not-allowed"
            >
              <span>{t("header.start")}</span>
              <span className="inline-flex size-control shrink-0 items-center justify-center rounded-full bg-primary-soft text-primary transition-colors group-focus-within:bg-primary group-focus-within:text-on-primary group-hover:bg-primary group-hover:text-on-primary motion-reduce:transition-none pointer-coarse:size-control-touch">
                <ArrowRight className="size-5" />
              </span>
            </Button>
            <div
              id="public-access-notice"
              role="tooltip"
              className="pointer-events-none invisible absolute top-full right-0 z-50 mt-3 w-72 rounded-marketing-card bg-surface p-5 text-left text-ui leading-relaxed text-ink opacity-0 shadow-overlay transition-opacity duration-150 group-focus-within:visible group-focus-within:opacity-100 group-hover:visible group-hover:opacity-100 motion-reduce:transition-none"
            >
              {t("header.accessUnavailable")}
            </div>
          </div>
        </nav>

        <Button
          ref={menuButtonRef}
          variant="ghost"
          size="icon"
          selected={menuOpen}
          aria-label={t("common.navigation")}
          aria-controls="mobile-site-navigation"
          aria-expanded={menuOpen}
          onClick={() => {
            setMenuOpen((open) => !open);
          }}
          className="-mr-2 xl:hidden"
        >
          <Icon name={menuOpen ? "close" : "menu"} className="size-6" />
        </Button>
      </div>

      <nav
        id="mobile-site-navigation"
        aria-label={t("common.navigation")}
        hidden={!menuOpen}
        className="max-h-[calc(100dvh-4rem)] overflow-y-auto rounded-b-marketing-shell bg-surface xl:hidden"
      >
        <div className="grid gap-1 px-4 py-3 sm:px-6 lg:px-8">
          {sectionLinks.map(({ href, label }) => (
            <a
              key={href}
              href={href}
              onClick={() => {
                setMenuOpen(false);
              }}
              className="flex min-h-control items-center rounded-pill px-3 text-ui leading-ui font-medium text-ink no-underline hover:bg-surface-muted focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-primary pointer-coarse:min-h-control-touch"
            >
              {t(label)}
            </a>
          ))}
          <div className="rounded-2xl bg-surface-muted px-3 py-3">
            <Button
              variant="brand"
              size="brand"
              aria-disabled="true"
              aria-describedby="mobile-public-access-notice"
              className="group w-full cursor-not-allowed"
            >
              <span>{t("header.start")}</span>
              <span className="ml-auto inline-flex size-10 shrink-0 items-center justify-center rounded-full bg-primary-soft text-primary transition-colors group-hover:bg-primary group-hover:text-on-primary group-focus-visible:bg-primary group-focus-visible:text-on-primary motion-reduce:transition-none">
                <ArrowRight className="size-5" />
              </span>
            </Button>
            <p
              id="mobile-public-access-notice"
              className="mt-1 text-ui leading-relaxed text-text-muted"
            >
              {t("header.accessUnavailable")}
            </p>
          </div>
        </div>
      </nav>
    </header>
  );
}
