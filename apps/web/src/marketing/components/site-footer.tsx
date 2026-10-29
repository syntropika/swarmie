import { BrandLogo } from "@swarmie/ui-web";

export function SiteFooter() {
  return (
    <footer aria-label="Swarmie" className="bg-footer text-footer-text">
      <div className="mx-auto w-[min(1248px,calc(100%_-_48px))] py-9 md:w-[min(1248px,calc(100%_-_96px))] md:py-12">
        <div className="flex items-center justify-end">
          <BrandLogo tone="inverse" treatment="monochrome" className="w-2/3 max-w-sm" />
        </div>
      </div>
    </footer>
  );
}
