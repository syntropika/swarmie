import { SwarmieMark } from "@swarmie/ui-web";

export function SiteFooter() {
  return (
    <footer className="bg-footer text-footer-text [&_:focus-visible]:outline-offset-4 [&_:focus-visible]:outline-footer-text">
      <div className="mx-auto w-[min(1248px,calc(100%_-_48px))] py-9 md:w-[min(1248px,calc(100%_-_96px))] md:py-12">
        <div className="flex items-center justify-between gap-4 text-wordmark leading-none font-semibold tracking-brand">
          swarmie
          <SwarmieMark tone="footer" className="size-14 md:size-28" />
        </div>
      </div>
    </footer>
  );
}
