import { Chip as HeroChip } from "@heroui/react";
import type { ChipProps as HeroChipProps, ChipLabelProps } from "@heroui/react";
import type { JSX } from "react";

export type BrandColor = "mint" | "lilac" | "peach" | "mustard";
export type ChipProps<E extends keyof JSX.IntrinsicElements = "span"> = Omit<
  Parameters<typeof HeroChip<E>>[0],
  "color"
> & {
  color?: HeroChipProps<E>["color"] | BrandColor;
};
function isBrandColor(color: ChipProps["color"]): color is BrandColor {
  return color === "mint" || color === "lilac" || color === "peach" || color === "mustard";
}
/** HeroUI owns chip geometry; approved brand accents supply optional category colors. */
function Root<E extends keyof JSX.IntrinsicElements = "span">({
  color = "default",
  ...props
}: ChipProps<E>) {
  // SAFETY: color is the only changed prop and is mapped back to a vendor color; the remaining generic props are unchanged.
  // oxlint-disable-next-line typescript/no-unsafe-type-assertion -- TypeScript cannot reconstruct a generic intrinsic props intersection after omitting color.
  const vendorProps = { ...props, color: isBrandColor(color) ? "default" : color } as Parameters<
    typeof HeroChip<E>
  >[0];
  return (
    <HeroChip<E> {...vendorProps} data-brand-color={isBrandColor(color) ? color : undefined} />
  );
}
// oxlint-disable-next-line no-redeclare -- The vendor API pairs a value with its compound prop type namespace.
export const Chip = Object.assign(Root, { Root, Label: HeroChip.Label });
export { Root as ChipRoot };
export { ChipLabel } from "@heroui/react";
export type ChipRootProps<E extends keyof JSX.IntrinsicElements = "span"> = ChipProps<E>;
export type Chip = { Props: ChipProps; RootProps: ChipRootProps; LabelProps: ChipLabelProps };
