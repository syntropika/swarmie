import inkCoralLogo from "../../../../../assets/brandkit/logos/swarmie-ink-coral.svg";
import inkLogo from "../../../../../assets/brandkit/logos/swarmie-mono-ink.svg";
import whiteLogo from "../../../../../assets/brandkit/logos/swarmie-mono-white.svg";
import whiteCoralLogo from "../../../../../assets/brandkit/logos/swarmie-white-coral.svg";
import inkCoralMark from "../../../../../assets/brandkit/marks/swarmie-mark-ink-coral.svg";
import inkMark from "../../../../../assets/brandkit/marks/swarmie-mark-mono-ink.svg";
import whiteMark from "../../../../../assets/brandkit/marks/swarmie-mark-mono-white.svg";
import whiteCoralMark from "../../../../../assets/brandkit/marks/swarmie-mark-white-coral.svg";
import { cn } from "../classes";
import { Reference, Section } from "./reference";

const topics = [
  ["brand", "Brand", "Canonical monochrome and orange-dot logos."],
  [
    "responsive-design",
    "Responsive Design",
    "Adaptive components, container queries and viewport breakpoints.",
  ],
  ["typography", "Typography", "Google Sans, type scales, weights and line height."],
  ["colors", "Colors", "Surfaces, text, actions and semantic status pairs."],
  ["spacing", "Spacing", "A shared scale for gaps, padding and layout rhythm."],
  ["shadows", "Shadows", "Field, surface and overlay elevation with rendered examples."],
  ["motion", "Motion", "Control feedback, measured transitions, easing and reduced motion."],
  ["icons", "Icons", "Available shared icons, sizing and accessible actions."],
] as const;

export function FoundationOverview() {
  return (
    <Reference
      title="Design foundations"
      intro="The visual rules underneath components. Atoms consume these rules; molecules compose the resulting controls across web and desktop."
    >
      <Section
        title="Explore the base"
        description="Each reference includes actual CSS samples, token names and examples of when to use them."
      >
        <nav aria-label="Foundation topics" className="divide-y divide-border">
          {topics.map(([id, title, description]) => (
            <a
              key={id}
              href={`?path=/story/foundations--${id}`}
              target="_top"
              className="grid gap-2 rounded-small py-4 text-primary focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-primary @lg:grid-cols-3"
            >
              <span className="text-body font-medium underline underline-offset-4">{title}</span>
              <span className="text-ui text-ink @lg:col-span-2">{description}</span>
            </a>
          ))}
        </nav>
      </Section>
      <Section title="One system, shared building blocks">
        <p className="max-w-3xl text-body">
          Fonts, semantic tokens and theme definitions belong to @swarmie/config. Atoms expose
          HeroUI controls. Molecules compose those controls into fields, menus and application
          patterns. Web and desktop consume the same public components and typography scale.
        </p>
        <p className="max-w-3xl text-ui text-text-muted">
          Application features own content, data and layout. Native keeps its platform renderer and
          its own qualification; it does not introduce another web component collection.
        </p>
      </Section>
    </Reference>
  );
}

const brandVariants = [
  {
    name: "Monochrome · dark ink",
    logo: inkLogo,
    mark: inkMark,
    theme: "light",
    group: "monochrome",
  },
  {
    name: "Monochrome · white",
    logo: whiteLogo,
    mark: whiteMark,
    theme: "dark",
    group: "monochrome",
  },
  {
    name: "Dark ink + orange dot",
    logo: inkCoralLogo,
    mark: inkCoralMark,
    theme: "light",
    group: "orange",
  },
  {
    name: "White + orange dot",
    logo: whiteCoralLogo,
    mark: whiteCoralMark,
    theme: "dark",
    group: "orange",
  },
] as const;

function brandPreview(theme: "light" | "dark") {
  return cn(
    "grid gap-6 rounded-panel p-6 text-ink @lg:p-8",
    theme === "light" ? "border border-border-subtle bg-surface-muted" : "bg-canvas",
  );
}

export function BrandReference() {
  return (
    <Reference
      title="Brand"
      intro="Canonical brandkit artwork, grouped by color treatment and logo format. Preserve the original proportions and leave clear space around each asset."
    >
      <Section
        title="Monochrome logos"
        description="Dark ink on a muted surface, white on a dark container. Both presentations use the same dimensions and spacing."
      >
        <div className="grid gap-6 @lg:grid-cols-2">
          {brandVariants
            .filter((variant) => variant.group === "monochrome")
            .map((variant) => (
              <figure
                key={variant.name}
                data-theme={variant.theme}
                className={brandPreview(variant.theme)}
              >
                <figcaption className="text-label">{variant.name}</figcaption>
                <img
                  src={variant.logo}
                  alt={`Swarmie ${variant.name} full logo`}
                  className="h-auto w-full max-w-xs"
                />
              </figure>
            ))}
        </div>
      </Section>
      <Section
        title="Orange-dot logos"
        description="The same dark and white versions with the canonical orange (coral) dot."
      >
        <div className="grid gap-6 @lg:grid-cols-2">
          {brandVariants
            .filter((variant) => variant.group === "orange")
            .map((variant) => (
              <figure
                key={variant.name}
                data-theme={variant.theme}
                className={brandPreview(variant.theme)}
              >
                <figcaption className="text-label">{variant.name}</figcaption>
                <img
                  src={variant.logo}
                  alt={`Swarmie ${variant.name} full logo`}
                  className="h-auto w-full max-w-xs"
                />
              </figure>
            ))}
        </div>
      </Section>
      <Section
        title="Mark and full logo"
        description="The standalone symbol and the complete logo containing the swarmie wordmark, in monochrome and orange-dot treatments."
      >
        <div className="grid gap-6">
          {brandVariants.map((variant) => (
            <div
              key={variant.name}
              data-theme={variant.theme}
              className={brandPreview(variant.theme)}
            >
              <h3 className="text-label">{variant.name}</h3>
              <div className="grid gap-6 @lg:grid-cols-2">
                <figure className="grid content-start gap-4">
                  <figcaption className="text-caption text-text-muted">Symbol only</figcaption>
                  <img
                    src={variant.mark}
                    alt={`Swarmie ${variant.name} standalone mark`}
                    className="size-20 object-contain"
                  />
                </figure>
                <figure className="grid content-start gap-4">
                  <figcaption className="text-caption text-text-muted">Symbol + swarmie</figcaption>
                  <img
                    src={variant.logo}
                    alt={`Swarmie ${variant.name} full logo comparison`}
                    className="h-auto w-full max-w-xs"
                  />
                </figure>
              </div>
            </div>
          ))}
        </div>
      </Section>
    </Reference>
  );
}
