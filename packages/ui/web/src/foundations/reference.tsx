import type { ReactNode } from "react";

import { cn } from "../classes";

export function Reference({
  title,
  children,
  intro,
}: {
  title: string;
  intro: string;
  children: ReactNode;
}) {
  return (
    <main className="@container mx-auto grid w-full max-w-6xl gap-10 p-4 font-sans text-ink sm:p-8">
      <header className="max-w-3xl">
        <h1 className="font-brand text-headline">{title}</h1>
        <p className="mt-3 text-body text-text-muted">{intro}</p>
      </header>
      {children}
    </main>
  );
}

export function Section({
  title,
  children,
  description,
  className,
}: {
  title: string;
  children: ReactNode;
  description?: string;
  className?: string;
}) {
  return (
    <section className={cn("grid min-w-0 gap-4", className)}>
      <header>
        <h2 className="font-brand text-title">{title}</h2>
        {description !== undefined && description !== "" ? (
          <p className="mt-2 max-w-3xl text-ui text-text-muted">{description}</p>
        ) : null}
      </header>
      {children}
    </section>
  );
}
