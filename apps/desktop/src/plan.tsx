import { Disclosure } from "~/src/components/ui/disclosure";

function record(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
function display(value: unknown, fallback: string): string {
  if (typeof value === "string") {
    return value;
  }
  if (typeof value === "number" || typeof value === "boolean") {
    return String(value);
  }
  return fallback;
}
function steps(value: unknown) {
  let items: unknown = value;
  if (record(value)) {
    items = value.plan ?? value.steps ?? value.entries;
  }
  if (!Array.isArray(items)) {
    return [];
  }
  const occurrences = new Map<string, number>();
  return items.map((item: unknown) => {
    const data = record(item) ? item : {};
    const text =
      typeof item === "string"
        ? item
        : display(data.text ?? data.step ?? data.title ?? data.content ?? data.description, "Step");
    const status = display(data.status, "").replaceAll("_", " ");
    const identity = display(data.id, JSON.stringify(item) ?? text);
    const occurrence = occurrences.get(identity) ?? 0;
    occurrences.set(identity, occurrence + 1);
    return { key: `${identity}:${occurrence}`, text, status };
  });
}
export function Plan({ value }: { value: unknown }) {
  const items = steps(value);
  let content = <pre>{JSON.stringify(value, null, 2)}</pre>;
  if (typeof value === "string") {
    content = <p>{value}</p>;
  } else if (items.length > 0) {
    content = (
      <ol className="plan-list">
        {items.map((item) => (
          <li key={item.key}>
            <span>{item.text}</span>
            {item.status.length > 0 && <small className="muted">{item.status}</small>}
          </li>
        ))}
      </ol>
    );
  }
  return (
    <div className="tool-activity">
      <Disclosure defaultOpen>
        <Disclosure.Summary>Plan</Disclosure.Summary>
        {content}
      </Disclosure>
    </div>
  );
}
