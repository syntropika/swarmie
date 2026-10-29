import { cn } from "../classes";
import { Icon } from "./icon";
import {
  AlibabaLogo,
  ClaudeLogo,
  CopilotLogo,
  GeminiLogo,
  KimiLogo,
  OpenAILogo,
} from "./model-logos";

const marks = {
  openai: OpenAILogo,
  claude: ClaudeLogo,
  anthropic: ClaudeLogo,
  gemini: GeminiLogo,
  kimi: KimiLogo,
  alibaba: AlibabaLogo,
  "github-copilot": CopilotLogo,
};

export function ProviderLogo({
  providerId,
  className,
}: {
  providerId: string | undefined;
  className?: string;
}) {
  const markClassName = cn("size-(--spacing-menu-icon) shrink-0", className);
  const Mark = Object.entries(marks).find(([id]) => id === providerId)?.[1];
  return Mark ? (
    <Mark aria-hidden="true" focusable="false" className={markClassName} />
  ) : (
    <Icon
      name={providerId === "local" || providerId === "ollama" ? "screen" : "agent"}
      className={markClassName}
    />
  );
}
