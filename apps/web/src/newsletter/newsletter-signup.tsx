import { createContext, useContext, useId, useMemo, useState } from "react";
import type { ReactNode } from "react";

import { NewsletterForm } from "~/src/newsletter/components/newsletter-form";
import { useNewsletter } from "~/src/newsletter/hooks/use-newsletter";

type NewsletterContextValue = {
  newsletter: ReturnType<typeof useNewsletter>;
  activeForm: string | null;
  setActiveForm: (id: string) => void;
};

const NewsletterContext = createContext<NewsletterContextValue | null>(null);

/** Provide submission state to the public signup form. */
export function NewsletterSignupProvider({ children }: { children: ReactNode }) {
  const newsletter = useNewsletter();
  const [activeForm, setActiveForm] = useState<string | null>(null);
  const value = useMemo(
    () => ({ newsletter, activeForm, setActiveForm }),
    [newsletter, activeForm],
  );
  return <NewsletterContext value={value}>{children}</NewsletterContext>;
}

export function NewsletterSignup() {
  const context = useContext(NewsletterContext);
  const id = useId();
  if (!context) {
    throw new Error("NewsletterSignupProvider is required");
  }
  return (
    <NewsletterForm
      {...context.newsletter}
      subscribe={(email, website) => {
        context.setActiveForm(id);
        context.newsletter.subscribe(email, website);
      }}
      announce={context.activeForm === id}
    />
  );
}
