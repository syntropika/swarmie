import { Disclosure } from "@/components/ui/disclosure";
export function Plan({ value }: { value: unknown }) {
  const data = value as Record<string, unknown>;
  const items = (
    Array.isArray(value) ? value : (data?.plan ?? data?.steps ?? data?.entries)
  ) as unknown[] | undefined;
  return (
    <Disclosure className="tool-activity" defaultOpen>
      <Disclosure.Summary>Plan</Disclosure.Summary>
      {typeof value === "string" ? (
        <p>{value}</p>
      ) : items?.length ? (
        <ol className="plan-list">
          {items.map((item, i) => {
            const step = item as Record<string, unknown>;
            return (
              <li key={i}>
                <span>
                  {typeof item === "string"
                    ? item
                    : String(
                        step.text ??
                          step.step ??
                          step.title ??
                          step.content ??
                          step.description ??
                          "Step",
                      )}
                </span>
                {step.status != null && (
                  <small className="muted">
                    {String(step.status).replaceAll("_", " ")}
                  </small>
                )}
              </li>
            );
          })}
        </ol>
      ) : (
        <pre>{JSON.stringify(value, null, 2)}</pre>
      )}
    </Disclosure>
  );
}
