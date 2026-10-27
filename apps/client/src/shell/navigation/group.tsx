import { ChevronDown } from "lucide-react";
import type { ReactNode } from "react";

export function NavigationGroup({
  title,
  folded,
  onToggle,
  children,
}: {
  title: string;
  folded: boolean;
  onToggle: () => void;
  children: ReactNode;
}) {
  return (
    <section className="mb-5">
      <div className="mb-1 flex h-7 items-center gap-1 px-1">
        <button
          type="button"
          aria-expanded={!folded}
          onClick={onToggle}
          className="flex flex-1 items-center gap-1 text-left text-chrome text-muted hover:text-foreground"
        >
          {title}
          <ChevronDown className={`size-3 ${folded ? "-rotate-90" : ""}`} />
        </button>
      </div>
      {folded ? null : children}
    </section>
  );
}
