import { Button, Icon, BrandLogo, Link, Tooltip } from "@swarmie/ui-web";
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
        <Link
          href="#top"
          aria-label={t("common.home")}
          onClick={() => {
            setMenuOpen(false);
          }}
          className="shrink-0"
        >
          <BrandLogo className="w-36 sm:w-44" />
        </Link>

        <nav aria-label={t("common.navigation")} className="hidden items-center gap-6 xl:flex">
          {sectionLinks.map(({ href, label }) => (
            <Link key={href} href={href}>
              {t(label)}
            </Link>
          ))}
          <Tooltip>
            <Button variant="tertiary" size="md" aria-disabled="true" className="ml-3">
              <span>{t("header.start")}</span>
              <ArrowRight />
            </Button>
            <Tooltip.Content placement="bottom" className="max-w-72">
              {t("header.accessUnavailable")}
            </Tooltip.Content>
          </Tooltip>
        </nav>

        <Button
          ref={menuButtonRef}
          variant="tertiary"
          isIconOnly
          aria-label={t("common.navigation")}
          aria-controls="mobile-site-navigation"
          aria-expanded={menuOpen}
          onClick={() => {
            setMenuOpen((open) => !open);
          }}
          className="-mr-2 xl:hidden"
        >
          <Icon name={menuOpen ? "close" : "menu"} />
        </Button>
      </div>

      <nav
        id="mobile-site-navigation"
        aria-label={t("common.navigation")}
        hidden={!menuOpen}
        className="max-h-[calc(100dvh-6rem)] overflow-y-auto rounded-b-marketing-shell bg-surface xl:hidden"
      >
        <div className="grid gap-4 px-4 py-3 sm:px-6 lg:px-8">
          {sectionLinks.map(({ href, label }) => (
            <Link
              key={href}
              href={href}
              onClick={() => {
                setMenuOpen(false);
              }}
            >
              {t(label)}
            </Link>
          ))}
          <div className="rounded-panel bg-surface-muted px-3 py-3">
            <Button
              variant="tertiary"
              size="md"
              aria-disabled="true"
              aria-describedby="mobile-public-access-notice"
              className="w-full"
            >
              <span>{t("header.start")}</span>
              <ArrowRight className="ml-auto" />
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
