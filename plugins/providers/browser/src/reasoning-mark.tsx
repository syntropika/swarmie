import { ProviderLogo, ReasoningIndicator } from "@swarmie/ui-web";

/** The Composer's provider-and-ring composition in a compact settings placement. */
export function ReasoningMark({
  providerId,
  levels,
  value,
}: {
  providerId: string;
  levels: readonly string[];
  value: string;
}) {
  return (
    <span
      aria-hidden="true"
      data-slot="reasoning-mark"
      className="relative grid size-5 shrink-0 place-items-center"
    >
      <ReasoningIndicator
        levels={levels}
        value={value}
        className="pointer-events-none absolute inset-0 m-0 size-full overflow-visible"
      />
      <ProviderLogo providerId={providerId} className="m-0 size-2.5" />
    </span>
  );
}
