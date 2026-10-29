import { useLocale } from "~/src/platform/i18n/provider";

const steps = [
  {
    title: "marketing.scenarioStepOneTitle",
    description: "marketing.scenarioStepOneDescription",
    visual: "report",
    visualColor: "bg-peach/65",
    position: "lg:col-span-5 lg:col-start-1",
  },
  {
    title: "marketing.scenarioStepTwoTitle",
    description: "marketing.scenarioStepTwoDescription",
    visual: "evaluation",
    visualColor: "bg-primary-soft",
    position: "lg:col-span-5 lg:col-start-8 lg:mt-20",
  },
  {
    title: "marketing.scenarioStepThreeTitle",
    description: "marketing.scenarioStepThreeDescription",
    visual: "pod",
    visualColor: "bg-mint/75",
    position: "lg:col-span-5 lg:col-start-2",
  },
  {
    title: "marketing.scenarioStepFourTitle",
    description: "marketing.scenarioStepFourDescription",
    visual: "review",
    visualColor: "bg-butter/75",
    position: "lg:col-span-5 lg:col-start-8 lg:mt-20",
  },
] as const;

/** An illustrative software-factory journey, not a simulated product workflow. */
export function ScenarioSection() {
  const { t } = useLocale();

  return (
    <section
      id="factory"
      aria-labelledby="scenario-title"
      className="mx-auto my-16 w-[min(1248px,calc(100%_-_48px))] scroll-mt-24 md:my-24 md:w-[min(1248px,calc(100%_-_96px))]"
    >
      <div className="overflow-hidden rounded-marketing-shell bg-inverse px-6 py-12 sm:px-9 md:px-14 md:py-16 lg:px-16">
        <div className="grid gap-6 md:grid-cols-[minmax(0,1.1fr)_minmax(0,0.9fr)] md:items-end md:gap-14">
          <h2
            id="scenario-title"
            className="max-w-[620px] text-marketing-section font-medium text-balance text-inverse-text"
          >
            {t("marketing.scenarioTitle")}
          </h2>
          <p className="max-w-[52ch] text-lg leading-relaxed text-pretty text-inverse-muted">
            {t("marketing.scenarioDescription")}
          </p>
        </div>

        <div className="relative mt-14 lg:mt-20">
          <svg
            aria-hidden="true"
            viewBox="0 0 1200 1000"
            preserveAspectRatio="none"
            className="pointer-events-none absolute inset-0 hidden size-full text-inverse-link/80 lg:block"
          >
            <path
              d="M395 150 C590 150 600 300 840 300 C1110 300 1070 590 370 590 C130 590 260 840 820 840"
              fill="none"
              stroke="currentColor"
              strokeWidth="2.5"
              strokeDasharray="6 9"
              strokeLinecap="round"
            />
          </svg>
          <ol className="relative grid gap-6 border-l border-inverse-link/70 pl-7 lg:grid-cols-12 lg:gap-y-12 lg:border-0 lg:pl-0">
            {steps.map(({ title, description, visual, visualColor, position }, index) => (
              <li
                key={title}
                data-motion-scene="factory-step"
                className={`relative min-w-0 overflow-visible rounded-marketing-card bg-surface text-ink shadow-raised ${position}`}
              >
                <span
                  aria-hidden="true"
                  className="absolute top-8 -left-10 flex size-6 items-center justify-center rounded-full bg-inverse-link text-xs font-semibold text-inverse lg:hidden"
                >
                  {index + 1}
                </span>
                <div className="overflow-hidden rounded-marketing-card">
                  <div
                    className={`flex h-44 items-center justify-center px-5 sm:h-48 ${visualColor}`}
                  >
                    <StageVisual stage={visual} />
                  </div>
                  <div className="px-6 pt-6 pb-7 break-words sm:px-8 sm:pt-7 sm:pb-8">
                    <div className="mb-5 flex items-center gap-3">
                      <span className="text-xs font-semibold text-ink tabular-nums">
                        {String(index + 1).padStart(2, "0")}
                      </span>
                      <span aria-hidden="true" className="h-px w-10 bg-border" />
                    </div>
                    <h3 className="text-2xl font-semibold text-balance text-ink">{t(title)}</h3>
                    <p className="mt-3 max-w-[44ch] leading-relaxed text-pretty text-text-muted">
                      {t(description)}
                    </p>
                  </div>
                </div>
              </li>
            ))}
          </ol>
        </div>
      </div>
    </section>
  );
}

function StageVisual({ stage }: { stage: (typeof steps)[number]["visual"] }) {
  return (
    <svg aria-hidden="true" viewBox="0 0 400 160" className="h-full w-full max-w-[440px]">
      {stage === "report" && (
        <>
          <rect x="40" y="24" width="266" height="113" rx="17" className="fill-surface" />
          <circle cx="67" cy="52" r="12" className="fill-peach" />
          <path
            d="M62 52h10m-5-5v10M92 48h101M92 58h68"
            className="stroke-ink"
            fill="none"
            strokeLinecap="round"
            strokeWidth="2"
          />
          <path
            d="M64 88h176M64 102h151M64 116h97"
            className="stroke-drawing-line"
            fill="none"
            strokeLinecap="round"
            strokeWidth="2"
          />
          <g data-motion-part="factory-report-signal">
            <rect x="269" y="79" width="86" height="58" rx="14" className="fill-primary-soft" />
            <circle cx="291" cy="100" r="7" className="fill-primary" />
            <path
              d="M308 98h28M283 117h53"
              className="stroke-primary"
              fill="none"
              strokeLinecap="round"
              strokeWidth="2"
            />
          </g>
        </>
      )}
      {stage === "evaluation" && (
        <>
          <rect x="16" y="54" width="70" height="52" rx="15" className="fill-surface" />
          <circle cx="39" cy="76" r="7" className="fill-primary" />
          <path
            d="M54 74h17M30 91h42"
            className="stroke-ink"
            fill="none"
            strokeLinecap="round"
            strokeWidth="2"
          />
          <path
            d="M86 80h36m0 0V43h24m-24 37v37h24M260 43h28v37h29m-57 37h28V80"
            className="stroke-primary"
            fill="none"
            strokeLinecap="round"
            strokeLinejoin="round"
            strokeWidth="2"
          />
          <circle cx="122" cy="80" r="5" className="fill-primary" />
          <g data-motion-part="factory-route-direct">
            <rect x="146" y="20" width="114" height="46" rx="14" className="fill-surface" />
            <path
              d="M162 39h75M162 49h51"
              className="stroke-ink"
              fill="none"
              strokeLinecap="round"
              strokeWidth="2"
            />
          </g>
          <g data-motion-part="factory-route-team">
            <rect x="146" y="94" width="114" height="46" rx="14" className="fill-surface" />
            <circle cx="166" cy="117" r="9" className="fill-peach" />
            <circle cx="180" cy="117" r="9" className="fill-lilac" />
            <path
              d="M197 113h46M197 123h33"
              className="stroke-ink"
              fill="none"
              strokeLinecap="round"
              strokeWidth="2"
            />
          </g>
          <circle cx="288" cy="80" r="5" className="fill-primary" />
          <g data-motion-part="factory-route-result">
            <rect x="317" y="51" width="67" height="58" rx="15" className="fill-primary" />
            <path
              d="M334 71h34M334 81h34M334 91h24"
              className="stroke-surface"
              fill="none"
              strokeLinecap="round"
              strokeWidth="2"
            />
          </g>
        </>
      )}
      {stage === "pod" && (
        <>
          <rect x="15" y="52" width="91" height="74" rx="15" className="fill-surface" />
          <circle cx="40" cy="77" r="7" className="fill-primary" />
          <path
            d="M55 75h31M31 96h55M31 108h39"
            className="stroke-ink"
            fill="none"
            strokeLinecap="round"
            strokeWidth="2"
          />
          <path
            d="M108 89h34m-10-10 10 10-10 10"
            className="stroke-primary"
            fill="none"
            strokeLinecap="round"
            strokeLinejoin="round"
            strokeWidth="2.5"
          />
          <rect x="155" y="16" width="230" height="130" rx="20" className="fill-inverse" />
          <path d="M155 49h230" className="stroke-inverse-link" strokeWidth="1.5" />
          <circle cx="175" cy="33" r="3" className="fill-peach" />
          <circle cx="187" cy="33" r="3" className="fill-butter" />
          <circle cx="199" cy="33" r="3" className="fill-mint" />
          <path
            d="m179 74 9 9-9 9m21 0h27m-27-18h97m-97 36h78m-78 14h43"
            className="stroke-inverse-link"
            fill="none"
            strokeLinecap="round"
            strokeLinejoin="round"
            strokeWidth="2"
          />
          <g data-motion-part="factory-pod-work">
            <rect x="307" y="69" width="54" height="55" rx="11" className="fill-primary-soft" />
            <path
              d="M318 83h28M318 94h28M318 105h18"
              className="stroke-primary"
              fill="none"
              strokeLinecap="round"
              strokeWidth="2"
            />
          </g>
        </>
      )}
      {stage === "review" && (
        <>
          <rect x="28" y="24" width="260" height="112" rx="18" className="fill-surface" />
          <path d="M28 54h260" className="stroke-border" strokeWidth="1.5" />
          <circle cx="48" cy="39" r="3" className="fill-peach" />
          <circle cx="60" cy="39" r="3" className="fill-butter" />
          <circle cx="72" cy="39" r="3" className="fill-mint" />
          <path
            d="M51 75h85M51 91h119M51 107h104"
            className="stroke-ink"
            fill="none"
            strokeLinecap="round"
            strokeWidth="2"
          />
          <path
            d="M187 75h55M187 91h69M187 107h42"
            className="stroke-primary"
            fill="none"
            strokeLinecap="round"
            strokeWidth="2"
          />
          <path
            d="M281 80h26m-8-8 8 8-8 8"
            className="stroke-primary"
            fill="none"
            strokeLinecap="round"
            strokeLinejoin="round"
            strokeWidth="2"
          />
          <g data-motion-part="factory-review-handoff">
            <rect x="310" y="48" width="67" height="69" rx="16" className="fill-primary-soft" />
            <circle cx="343" cy="72" r="12" className="fill-peach" />
            <path
              d="M327 97h32M333 106h20"
              className="stroke-ink"
              fill="none"
              strokeLinecap="round"
              strokeWidth="2"
            />
          </g>
        </>
      )}
    </svg>
  );
}
