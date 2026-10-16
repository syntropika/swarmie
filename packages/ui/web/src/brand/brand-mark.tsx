// oxlint-disable-next-line typescript/triple-slash-reference -- Carry the shared ambient asset contract into consumers without requiring Vite globals.
/// <reference path="../assets.d.ts" />

import type { ComponentPropsWithRef } from "react";

import inkCoralLogo from "../../../../../assets/brandkit/logos/swarmie-ink-coral.svg?url";
import inkLogo from "../../../../../assets/brandkit/logos/swarmie-mono-ink.svg?url";
import whiteLogo from "../../../../../assets/brandkit/logos/swarmie-mono-white.svg?url";
import whiteCoralLogo from "../../../../../assets/brandkit/logos/swarmie-white-coral.svg?url";
import inkCoralMark from "../../../../../assets/brandkit/marks/swarmie-mark-ink-coral.svg?url";
import inkMark from "../../../../../assets/brandkit/marks/swarmie-mark-mono-ink.svg?url";
import whiteMark from "../../../../../assets/brandkit/marks/swarmie-mark-mono-white.svg?url";
import whiteCoralMark from "../../../../../assets/brandkit/marks/swarmie-mark-white-coral.svg?url";
import inkWordmark from "../../../../../assets/brandkit/wordmarks/swarmie-wordmark-ink.svg?url";
import whiteWordmark from "../../../../../assets/brandkit/wordmarks/swarmie-wordmark-white.svg?url";
import { cn } from "../classes";

export type BrandMarkProps = ComponentPropsWithRef<"span"> & {
  tone?: "primary" | "inverse";
  treatment?: "coral" | "monochrome";
};
export type BrandLogoProps = BrandMarkProps;
export type BrandWordmarkProps = Omit<BrandMarkProps, "treatment">;

const artwork = {
  mark: { coral: [inkCoralMark, whiteCoralMark], monochrome: [inkMark, whiteMark] },
  logo: { coral: [inkCoralLogo, whiteCoralLogo], monochrome: [inkLogo, whiteLogo] },
} as const;

function BrandArtwork({
  kind,
  tone = "primary",
  treatment = "coral",
  className,
  ...props
}: BrandMarkProps & { kind: keyof typeof artwork | "wordmark" }) {
  const [ink, white] =
    kind === "wordmark" ? [inkWordmark, whiteWordmark] : artwork[kind][treatment];
  const imageClass = kind === "mark" ? "size-full object-contain" : "h-auto w-full";
  return (
    <span {...props} aria-hidden="true" className={cn("inline-flex shrink-0", className)}>
      {tone === "primary" ? (
        <>
          <img src={ink} alt="" className={cn(imageClass, "dark:hidden")} />
          <img src={white} alt="" className={cn(imageClass, "hidden dark:block")} />
        </>
      ) : (
        <img src={white} alt="" className={imageClass} />
      )}
    </span>
  );
}

/** Decorative canonical symbol; its containing control supplies the accessible name. */
export function BrandMark(props: BrandMarkProps) {
  return <BrandArtwork {...props} kind="mark" />;
}

/** Canonical symbol and custom-lettered wordmark, loaded consistently in SSR and the browser. */
export function BrandLogo(props: BrandLogoProps) {
  return <BrandArtwork {...props} kind="logo" />;
}

/** Original monochrome lettering, extracted from the canonical logo without the symbol. */
export function BrandWordmark(props: BrandWordmarkProps) {
  return <BrandArtwork {...props} kind="wordmark" treatment="monochrome" />;
}
