import { ChromeButton, ProviderLogo, ReasoningIndicator } from "@swarmie/ui-web";

const noLevels: readonly string[] = [];

export function ModelTrigger({
  providerId,
  thinking,
  levels = noLevels,
  open,
  label,
  onPress,
}: {
  providerId: string | undefined;
  thinking: string;
  levels?: readonly string[];
  open: boolean;
  label: string;
  onPress?: () => void;
}) {
  return (
    <ChromeButton
      size="composerModel"
      aria-label={label}
      title={label}
      selected={open}
      {...(onPress === undefined ? {} : { onPress })}
    >
      <span
        data-slot="model-ring-frame"
        className="relative flex size-(--spacing-composer-send) shrink-0 items-center justify-center"
      >
        <ReasoningIndicator
          levels={levels}
          value={thinking}
          className="pointer-events-none absolute inset-0 m-0 size-full overflow-visible"
        />
        <ProviderLogo providerId={providerId} className="m-0 size-(--spacing-chrome-icon)" />
      </span>
    </ChromeButton>
  );
}
