import type { ReactNode } from "react";

import { useLocale } from "~/src/platform/i18n/provider";

/** These small scenes suggest product relationships without presenting released screens. */
function DrawingSurface({ children, color }: { children: ReactNode; color: string }) {
  return (
    <div
      aria-hidden="true"
      className={`flex h-48 items-center justify-center overflow-hidden p-4 sm:h-56 sm:p-6 lg:h-60 ${color}`}
    >
      <svg viewBox="0 0 420 176" className="h-full w-full max-w-[440px]" fill="none">
        {children}
      </svg>
    </div>
  );
}

/** Source cards retain visible connections to the context gathered from them. */
function MemoryDrawing() {
  return (
    <DrawingSurface color="bg-mint/35">
      <path
        d="M145 45c29 0 31 43 72 43m-65 5h65m-58 47c23 0 27-47 58-47"
        className="stroke-primary"
        strokeWidth="1.5"
        strokeLinecap="round"
        strokeDasharray="3 5"
      />
      <rect x="19" y="14" width="126" height="58" rx="14" className="fill-surface stroke-border" />
      <rect x="31" y="66" width="121" height="56" rx="14" className="fill-surface stroke-border" />
      <rect x="39" y="112" width="120" height="51" rx="14" className="fill-surface stroke-border" />
      <rect x="32" y="28" width="23" height="29" rx="5" className="fill-butter" />
      <path
        d="M39 36h9m-9 6h9m-9 6h6"
        className="stroke-ink"
        strokeWidth="1.4"
        strokeLinecap="round"
      />
      <path d="M66 37h62m-62 9h46" className="stroke-ink" strokeWidth="2" strokeLinecap="round" />
      <circle cx="54" cy="94" r="10" className="fill-lilac" />
      <path
        d="M48 96c0-5 12-5 12 0m-6-7a2.5 2.5 0 1 0 0 5 2.5 2.5 0 0 0 0-5"
        className="stroke-ink"
        strokeWidth="1.4"
      />
      <path d="M74 90h58m-58 9h42" className="stroke-ink" strokeWidth="2" strokeLinecap="round" />
      <rect x="51" y="123" width="22" height="26" rx="5" className="fill-peach" />
      <path
        d="M57 130h10m-10 6h10m-10 6h7"
        className="stroke-ink"
        strokeWidth="1.4"
        strokeLinecap="round"
      />
      <path d="M84 132h57m-57 9h38" className="stroke-ink" strokeWidth="2" strokeLinecap="round" />
      <circle cx="217" cy="89" r="5" className="fill-primary" />
      <path d="M222 89h17" className="stroke-primary" strokeWidth="1.5" />
      <rect
        x="239"
        y="23"
        width="160"
        height="130"
        rx="20"
        className="fill-surface stroke-primary"
        strokeWidth="1.4"
      />
      <rect x="255" y="39" width="49" height="16" rx="8" className="fill-mint" />
      <path d="M266 47h27" className="stroke-ink" strokeWidth="1.6" strokeLinecap="round" />
      <path
        d="M255 70h113m-113 12h95m-95 12h105"
        className="stroke-ink"
        strokeWidth="2"
        strokeLinecap="round"
      />
      <path d="M255 110h128" className="stroke-border" strokeWidth="1" />
      <circle cx="263" cy="131" r="5" className="fill-butter" />
      <circle cx="282" cy="131" r="5" className="fill-lilac" />
      <circle cx="301" cy="131" r="5" className="fill-peach" />
      <path d="M317 131h55" className="stroke-primary" strokeWidth="1.5" strokeLinecap="round" />
    </DrawingSurface>
  );
}

/** Circular people and a squared Agent contribute to one conversation. */
function CollaborationDrawing() {
  return (
    <DrawingSurface color="bg-lilac/35">
      <path
        d="M88 51h44m-44 75h44"
        className="stroke-primary"
        strokeWidth="1.5"
        strokeDasharray="3 5"
      />
      <circle cx="57" cy="51" r="29" className="fill-surface stroke-border" />
      <circle cx="57" cy="44" r="8" className="fill-mint" />
      <path
        d="M42 64c3-12 27-12 30 0"
        className="stroke-ink"
        strokeWidth="1.6"
        strokeLinecap="round"
      />
      <rect
        x="28"
        y="97"
        width="58"
        height="58"
        rx="17"
        className="fill-surface stroke-primary"
        strokeWidth="1.4"
      />
      <path
        d="M47 126h20m-10-10v20"
        className="stroke-primary"
        strokeWidth="1.8"
        strokeLinecap="round"
      />
      <circle cx="132" cy="51" r="4" className="fill-primary" />
      <circle cx="132" cy="126" r="4" className="fill-primary" />
      <rect
        x="143"
        y="14"
        width="258"
        height="148"
        rx="21"
        className="fill-surface stroke-border"
      />
      <path d="M143 51h258" className="stroke-border" strokeWidth="1" />
      <circle cx="161" cy="33" r="5" className="fill-mint" />
      <rect x="175" y="29" width="83" height="8" rx="4" className="fill-surface-muted" />
      <circle cx="382" cy="33" r="3" className="fill-primary" />
      <circle cx="372" cy="33" r="3" className="fill-drawing-line" />
      <rect x="157" y="64" width="161" height="30" rx="14" className="fill-surface-muted" />
      <path
        d="M171 75h119m-119 9h84"
        className="stroke-ink"
        strokeWidth="1.8"
        strokeLinecap="round"
      />
      <rect x="216" y="100" width="170" height="31" rx="14" className="fill-primary-soft" />
      <path
        d="M231 111h134m-134 9h99"
        className="stroke-primary"
        strokeWidth="1.8"
        strokeLinecap="round"
      />
      <rect x="157" y="139" width="123" height="12" rx="6" className="fill-mint" />
      <path
        d="m167 145 3 3 5-6m9 3h79"
        className="stroke-ink"
        strokeWidth="1.4"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </DrawingSurface>
  );
}

/** The Task crosses into a bounded Pod with room for the Agent's tools. */
function PodDrawing() {
  return (
    <DrawingSurface color="bg-butter/35">
      <rect x="19" y="43" width="130" height="91" rx="18" className="fill-surface stroke-border" />
      <rect x="34" y="59" width="32" height="32" rx="9" className="fill-butter" />
      <path
        d="M43 69h14m-14 6h14m-14 6h9"
        className="stroke-ink"
        strokeWidth="1.6"
        strokeLinecap="round"
      />
      <path d="M76 66h58m-58 11h43" className="stroke-ink" strokeWidth="2" strokeLinecap="round" />
      <path
        d="M34 107h99m-99 11h69"
        className="stroke-drawing-line"
        strokeWidth="2"
        strokeLinecap="round"
      />
      <path
        d="M149 88h41m-9-8 9 8-9 8"
        className="stroke-primary"
        strokeWidth="1.6"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
      <rect
        x="204"
        y="13"
        width="192"
        height="150"
        rx="23"
        className="fill-surface stroke-primary"
        strokeWidth="1.5"
      />
      <rect
        x="212"
        y="21"
        width="176"
        height="134"
        rx="18"
        className="stroke-border"
        strokeWidth="1"
        strokeDasharray="3 5"
      />
      <path d="M204 50h192" className="stroke-border" strokeWidth="1" />
      <circle cx="224" cy="32" r="3" className="fill-peach" />
      <circle cx="236" cy="32" r="3" className="fill-butter" />
      <circle cx="248" cy="32" r="3" className="fill-mint" />
      <rect x="268" y="27" width="96" height="10" rx="5" className="fill-primary-soft" />
      <path
        d="M230 70h17m-17 12h17m-17 12h17"
        className="stroke-primary"
        strokeWidth="2"
        strokeLinecap="round"
      />
      <path
        d="M256 70h109m-109 12h88m-88 12h99"
        className="stroke-ink"
        strokeWidth="2"
        strokeLinecap="round"
      />
      <rect x="225" y="111" width="151" height="29" rx="10" className="fill-mint" />
      <path
        d="m239 120 6 5-6 5m17 0h74"
        className="stroke-ink"
        strokeWidth="1.8"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
      <circle cx="359" cy="125" r="4" className="fill-primary" />
    </DrawingSurface>
  );
}

/** An execution trail sits alongside the output a person can inspect. */
function OutcomeDrawing() {
  return (
    <DrawingSurface color="bg-peach/30">
      <rect x="18" y="17" width="204" height="142" rx="20" className="fill-surface stroke-border" />
      <path d="M47 52v68" className="stroke-primary" strokeWidth="1.4" />
      <circle cx="47" cy="43" r="7" className="fill-primary" />
      <circle cx="47" cy="88" r="7" className="fill-lilac stroke-primary" strokeWidth="1.3" />
      <circle cx="47" cy="132" r="7" className="fill-butter stroke-primary" strokeWidth="1.3" />
      <path
        d="M67 40h108m-108 11h74M67 84h120m-120 11h89m-89 33h109m-109 11h81"
        className="stroke-ink"
        strokeWidth="2"
        strokeLinecap="round"
      />
      <rect
        x="195"
        y="31"
        width="205"
        height="130"
        rx="20"
        className="fill-surface stroke-primary"
        strokeWidth="1.4"
      />
      <path d="M195 63h205" className="stroke-border" strokeWidth="1" />
      <rect x="212" y="45" width="78" height="8" rx="4" className="fill-primary-soft" />
      <circle cx="374" cy="49" r="4" className="fill-primary" />
      <rect x="211" y="78" width="173" height="28" rx="9" className="fill-surface-muted" />
      <path
        d="M223 88h137m-137 9h91"
        className="stroke-ink"
        strokeWidth="1.7"
        strokeLinecap="round"
      />
      <rect x="211" y="113" width="173" height="27" rx="9" className="fill-mint" />
      <path
        d="M223 123h119m-119 8h104"
        className="stroke-ink"
        strokeWidth="1.7"
        strokeLinecap="round"
      />
      <circle cx="372" cy="133" r="17" className="fill-surface stroke-primary" strokeWidth="1.4" />
      <path
        d="M379 141l10 10m-12-20a6 6 0 1 1-12 0 6 6 0 0 1 12 0Z"
        className="stroke-primary"
        strokeWidth="1.6"
        strokeLinecap="round"
      />
    </DrawingSurface>
  );
}

const features = [
  {
    title: "marketing.capabilityContextTitle",
    description: "marketing.capabilityContextDescription",
    Drawing: MemoryDrawing,
    layout: "lg:col-span-7",
  },
  {
    title: "marketing.capabilityCollaborationTitle",
    description: "marketing.capabilityCollaborationDescription",
    Drawing: CollaborationDrawing,
    layout: "lg:col-span-5",
  },
  {
    title: "marketing.capabilityExecutionTitle",
    description: "marketing.capabilityExecutionDescription",
    Drawing: PodDrawing,
    layout: "lg:col-span-5",
  },
  {
    title: "marketing.capabilityControlTitle",
    description: "marketing.capabilityControlDescription",
    Drawing: OutcomeDrawing,
    layout: "lg:col-span-7",
  },
] as const;

/** Product capabilities, kept distinct from the illustrative onboarding conversation. */
export function CapabilitiesSection() {
  const { t } = useLocale();

  return (
    <section
      id="capabilities"
      aria-labelledby="capabilities-title"
      className="scroll-mt-24 bg-canvas py-20 md:py-28"
    >
      <div className="mx-auto w-[min(1248px,calc(100%_-_48px))] md:w-[min(1248px,calc(100%_-_96px))]">
        <div className="grid gap-5 md:grid-cols-[minmax(0,0.85fr)_minmax(0,1fr)] md:gap-16">
          <h2
            id="capabilities-title"
            className="max-w-[560px] text-marketing-section font-medium text-balance"
          >
            {t("marketing.capabilitiesTitle")}
          </h2>
          <p className="max-w-[60ch] self-end text-lg leading-relaxed text-pretty text-text-muted">
            {t("marketing.capabilitiesDescription")}
          </p>
        </div>

        <div className="mt-12 grid gap-5 md:mt-16 md:grid-cols-2 md:gap-6 lg:grid-cols-12">
          {features.map(({ title, description, Drawing, layout }) => (
            <article
              key={title}
              className={`min-w-0 overflow-hidden rounded-4xl bg-surface shadow-raised ${layout}`}
            >
              <Drawing />
              <div className="px-6 pt-7 pb-8 sm:px-8 sm:pt-8 sm:pb-10">
                <h3 className="max-w-[25ch] text-2xl leading-tight font-semibold text-balance sm:text-3xl">
                  {t(title)}
                </h3>
                <p className="mt-4 max-w-[54ch] text-body leading-relaxed text-pretty text-text-muted">
                  {t(description)}
                </p>
              </div>
            </article>
          ))}
        </div>

        <div className="mt-6 flex flex-col gap-6 rounded-4xl bg-primary-soft px-6 py-7 sm:px-8 md:flex-row md:items-center md:justify-between md:gap-12 md:px-10 md:py-9">
          <h3 className="max-w-[20ch] text-xl leading-snug font-semibold text-balance sm:text-2xl">
            {t("marketing.capabilityAdaptabilityTitle")}
          </h3>
          <p className="max-w-[59ch] leading-relaxed text-pretty text-ink">
            {t("marketing.capabilityAdaptabilityDescription")}
          </p>
        </div>
      </div>
    </section>
  );
}
