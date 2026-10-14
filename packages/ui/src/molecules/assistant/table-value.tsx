import { useEffect, useState } from "react";

export type TableValueFormat =
  | { kind: "number"; options?: Intl.NumberFormatOptions }
  | { kind: "date"; options?: Intl.DateTimeFormatOptions }
  | { kind: "relative" };

/** Formatting composes through DataColumn.render; hosts retain ownership of the raw row. */
export function TableValue({
  value,
  format,
  locale = "en",
  relativeTo,
}: {
  value: number | Date;
  format: TableValueFormat;
  locale?: string;
  relativeTo?: number;
}) {
  const [now, setNow] = useState(() => relativeTo ?? Date.now());
  useEffect(() => {
    if (format.kind !== "relative" || relativeTo !== undefined) {
      return undefined;
    }
    const timer = setInterval(() => {
      setNow(Date.now());
    }, 1000);
    return () => {
      clearInterval(timer);
    };
  }, [format.kind, relativeTo]);
  const numeric = value instanceof Date ? value.getTime() : value;
  if (!Number.isFinite(numeric)) {
    return <span>—</span>;
  }
  if (format.kind === "number") {
    return <>{new Intl.NumberFormat(locale, format.options).format(numeric)}</>;
  }
  if (format.kind === "date") {
    return (
      <time dateTime={new Date(numeric).toISOString()}>
        {new Intl.DateTimeFormat(locale, format.options).format(numeric)}
      </time>
    );
  }
  const seconds = (numeric - (relativeTo ?? now)) / 1000;
  const units: readonly [Intl.RelativeTimeFormatUnit, number][] = [
    ["year", 31_536_000],
    ["month", 2_592_000],
    ["week", 604_800],
    ["day", 86_400],
    ["hour", 3600],
    ["minute", 60],
    ["second", 1],
  ];
  const [unit, size] = units.find(([, amount]) => Math.abs(seconds) >= amount) ?? ["second", 1];
  return (
    <time dateTime={new Date(numeric).toISOString()}>
      {new Intl.RelativeTimeFormat(locale, { numeric: "auto" }).format(
        Math.round(seconds / size),
        unit,
      )}
    </time>
  );
}
