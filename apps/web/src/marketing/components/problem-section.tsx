import { Icon } from "@swarmie/ui-web";
import type { IconName } from "@swarmie/ui-web";

import { useLocale } from "~/src/platform/i18n/provider";

const fragments = [
  {
    title: "marketing.problemPointOneTitle",
    description: "marketing.problemPointOneDescription",
    color: "bg-peach/65",
    kind: "brief",
  },
  {
    title: "marketing.problemPointTwoTitle",
    description: "marketing.problemPointTwoDescription",
    color: "bg-butter/65",
    kind: "handoff",
  },
  {
    title: "marketing.problemPointThreeTitle",
    description: "marketing.problemPointThreeDescription",
    color: "bg-lilac/65",
    kind: "history",
  },
] as const;

type FragmentKind = (typeof fragments)[number]["kind"];

const fragmentIcons = {
  brief: "file",
  handoff: "arrows-exchange",
  history: "history",
} satisfies Record<FragmentKind, IconName>;

function FragmentIcon({ kind }: { kind: FragmentKind }) {
  return <Icon name={fragmentIcons[kind]} className="size-6" />;
}

/** Separate windows and interrupted paths make the loss of context visible. */
function FragmentedWorkspace() {
  return (
    <div
      data-motion-scene="problem"
      className="mx-auto w-full max-w-[520px] rounded-marketing-shell bg-primary-soft/55 p-3 sm:p-6"
      aria-hidden="true"
    >
      <svg viewBox="0 0 520 284" className="h-auto w-full" fill="none">
        <path
          data-motion-part="problem-links"
          d="M162 112h41m23 0h43M250 163h31m21 0h39"
          className="stroke-primary"
          strokeWidth="2"
          strokeLinecap="round"
          strokeDasharray="4 6"
        />
        <circle cx="214" cy="112" r="4" className="fill-surface stroke-primary" strokeWidth="1.5" />
        <circle cx="292" cy="163" r="4" className="fill-surface stroke-primary" strokeWidth="1.5" />
        <g data-motion-part="problem-window">
          <rect
            x="24"
            y="31"
            width="184"
            height="132"
            rx="23"
            className="fill-surface stroke-border"
          />
          <circle cx="50" cy="57" r="8" className="fill-peach" />
          <path
            d="M68 54h80m-80 11h57"
            className="stroke-ink"
            strokeWidth="2.4"
            strokeLinecap="round"
          />
          <rect x="44" y="86" width="140" height="53" rx="15" className="fill-surface-muted" />
          <path
            d="M59 101h106m-106 11h85m-85 11h97"
            className="stroke-drawing-line"
            strokeWidth="2"
            strokeLinecap="round"
          />
        </g>
        <g data-motion-part="problem-window">
          <rect
            x="268"
            y="15"
            width="228"
            height="126"
            rx="23"
            className="fill-surface stroke-border"
          />
          <rect x="286" y="35" width="43" height="43" rx="12" className="fill-butter" />
          <path
            d="M298 48h19m-19 8h19m-19 8h13"
            className="stroke-ink"
            strokeWidth="2"
            strokeLinecap="round"
          />
          <path
            d="M341 47h129m-129 12h101"
            className="stroke-ink"
            strokeWidth="2.4"
            strokeLinecap="round"
          />
          <path
            d="M286 98h180m-180 13h142"
            className="stroke-drawing-line"
            strokeWidth="2"
            strokeLinecap="round"
          />
        </g>
        <g data-motion-part="problem-window">
          <rect
            x="169"
            y="157"
            width="204"
            height="110"
            rx="23"
            className="fill-surface stroke-border"
          />
          <path d="M169 193h204" className="stroke-border" strokeWidth="1" />
          <circle cx="192" cy="175" r="5" className="fill-lilac" />
          <path d="M207 175h83" className="stroke-ink" strokeWidth="2.4" strokeLinecap="round" />
          <path
            d="M193 215h154m-154 13h116m-116 13h139"
            className="stroke-drawing-line"
            strokeWidth="2"
            strokeLinecap="round"
          />
          <circle
            cx="343"
            cy="241"
            r="12"
            className="fill-surface stroke-primary"
            strokeWidth="1.5"
          />
          <path
            d="m349 249 8 8m-10-17a5 5 0 1 1-10 0 5 5 0 0 1 10 0Z"
            className="stroke-primary"
            strokeWidth="1.5"
            strokeLinecap="round"
          />
        </g>
      </svg>
    </div>
  );
}

/** Explain the context lost when software work moves between disconnected places. */
export function ProblemSection() {
  const { t } = useLocale();

  return (
    <section aria-labelledby="problem-title" className="bg-surface-muted/45 py-20 md:py-28">
      <div className="mx-auto w-[min(1248px,calc(100%_-_48px))] md:w-[min(1248px,calc(100%_-_96px))]">
        <div className="overflow-hidden rounded-marketing-shell bg-surface shadow-raised">
          <div className="grid items-center gap-10 px-6 py-10 sm:px-10 md:py-14 lg:grid-cols-[minmax(0,1fr)_minmax(0,0.95fr)] lg:gap-14 lg:px-14 lg:py-16">
            <div>
              <h2
                id="problem-title"
                className="max-w-[630px] text-marketing-section font-medium text-balance"
              >
                {t("marketing.problemTitle")}
              </h2>
              <p className="mt-6 max-w-[58ch] text-lg leading-relaxed text-pretty text-text-muted">
                {t("marketing.problemDescription")}
              </p>
            </div>
            <FragmentedWorkspace />
          </div>
          <ol className="grid md:grid-cols-3">
            {fragments.map(({ title, description, color, kind }) => (
              <li key={title} className="p-6 sm:p-8 lg:p-10">
                <span
                  className={`flex size-12 items-center justify-center rounded-panel text-ink ${color}`}
                >
                  <FragmentIcon kind={kind} />
                </span>
                <h3 className="mt-5 max-w-[22ch] text-xl leading-snug font-semibold text-balance">
                  {t(title)}
                </h3>
                <p className="mt-3 max-w-[42ch] leading-relaxed text-pretty text-text-muted">
                  {t(description)}
                </p>
              </li>
            ))}
          </ol>
        </div>
      </div>
    </section>
  );
}
