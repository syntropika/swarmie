import { useId, useState } from "react";
import type { ReactNode } from "react";

import { Badge } from "../../atoms/badge";
import { Button } from "../../atoms/button";
import { Icon } from "../../atoms/icon";
import { cn } from "../../classes";
import { chartGeometry } from "./chart-geometry";
import { AssistantPanel, AssistantProgress, ExecutionStatus, PanelHeading } from "./shared";
import type { ExecutionState, LabeledValue } from "./shared";
import { AssistantModal } from "./thread";

export type DataSort = { id: string; direction: "ascending" | "descending" } | null;

export type DataColumn<Row> = {
  id: string;
  label: string;
  render: (row: Row) => ReactNode;
  compare?: (left: Row, right: Row) => number;
};

/** Sorting is local presentation state. Rows retain host-provided stable identities. */
export function DataTable<Row>({
  rows,
  columns,
  rowId,
  caption,
  emptyLabel,
  sort: controlledSort,
  defaultSort = null,
  onSortChange,
  sortAnnouncement,
}: {
  rows: readonly Row[];
  columns: readonly DataColumn<Row>[];
  rowId: (row: Row) => string;
  caption: string;
  emptyLabel: string;
  sort?: DataSort;
  defaultSort?: DataSort;
  onSortChange?: (sort: DataSort) => void;
  sortAnnouncement?: (sort: DataSort) => string;
}) {
  const [localSort, setLocalSort] = useState<DataSort>(defaultSort);
  const sort = controlledSort === undefined ? localSort : controlledSort;
  const column = columns.find((entry) => entry.id === sort?.id);
  const ordered = [...rows];
  if (column?.compare && sort) {
    const { compare } = column;
    ordered.sort((left, right) =>
      sort.direction === "ascending" ? compare(left, right) : compare(right, left),
    );
  }
  return (
    <div className="overflow-x-auto rounded-panel border border-transparent">
      <output className="sr-only" aria-live="polite">
        {sortAnnouncement?.(sort)}
      </output>
      <table className="w-full text-left text-ui whitespace-nowrap">
        <caption className="border-b border-border bg-surface p-3 text-left font-medium">
          {caption}
        </caption>
        <thead className="bg-surface-muted">
          <tr>
            {columns.map((entry) => (
              <th
                key={entry.id}
                scope="col"
                aria-sort={sort?.id === entry.id ? sort.direction : undefined}
                className="px-3 py-2 font-medium"
              >
                {entry.compare ? (
                  <Button
                    size="sm"
                    variant="ghost"
                    className="-mx-2 px-2"
                    onClick={() => {
                      let next: DataSort = { id: entry.id, direction: "ascending" };
                      if (sort?.id === entry.id) {
                        next =
                          sort.direction === "ascending"
                            ? { id: entry.id, direction: "descending" }
                            : null;
                      }
                      if (controlledSort === undefined) {
                        setLocalSort(next);
                      }
                      onSortChange?.(next);
                    }}
                  >
                    {entry.label}
                    <Icon name="chevron" />
                  </Button>
                ) : (
                  entry.label
                )}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {ordered.map((row) => (
            <tr key={rowId(row)} className="border-t border-border">
              {columns.map((entry) => (
                <td key={entry.id} className="px-3 py-2 tabular-nums">
                  {entry.render(row)}
                </td>
              ))}
            </tr>
          ))}
          {ordered.length === 0 ? (
            <tr>
              <td colSpan={columns.length} className="p-3 text-text-muted">
                {emptyLabel}
              </td>
            </tr>
          ) : null}
        </tbody>
      </table>
    </div>
  );
}

/** Each numeric place has a ten-digit reel; CSS follows live reduced-motion changes. */
export function NumberTicker({
  value,
  label,
  locale = "en",
  format,
}: {
  value: number;
  label: string;
  locale?: string;
  format?: Intl.NumberFormatOptions;
}) {
  const formatted = new Intl.NumberFormat(locale, format).format(value);
  return (
    <div className="space-y-1">
      <p className="text-ui text-text-muted">{label}</p>
      <output className="inline-flex text-title tabular-nums">
        <span aria-hidden="true" className="inline-flex">
          {Array.from(
            new Intl.Segmenter(locale, { granularity: "grapheme" }).segment(formatted),
            (part) => part.segment,
          ).map((character, index) => {
            const digit = Number(character);
            // Digit places are stable positions in the formatted number, including repeated characters.
            return /[0-9]/u.test(character) ? (
              <svg
                key={`${formatted.length - index}`}
                viewBox="0 0 10 16"
                className="h-7 w-4 overflow-hidden"
              >
                <g
                  transform={`translate(0 ${-digit * 16})`}
                  className="transition-transform duration-500 ease-assistant-reel motion-reduce:transition-none"
                >
                  {[0, 1, 2, 3, 4, 5, 6, 7, 8, 9].map((number) => (
                    <text
                      key={number}
                      x="5"
                      y={number * 16 + 12}
                      textAnchor="middle"
                      className="fill-current"
                    >
                      {number}
                    </text>
                  ))}
                </g>
              </svg>
            ) : (
              <span key={`${formatted.length - index}`}>{character}</span>
            );
          })}
        </span>
        <span className="sr-only" aria-live="polite">
          {formatted}
        </span>
      </output>
    </div>
  );
}

export type ChartSeries = { id: string; label: string; values: readonly number[] };
export type ChartProps = {
  label: string;
  labels: readonly string[];
  series: readonly ChartSeries[];
  kind?: "line" | "area" | "bar";
  stacked?: boolean;
  locale?: string;
  visibleCount?: number;
  delta?: string;
  trend?: "up" | "down" | "flat";
};
const seriesClasses = [
  "fill-primary stroke-primary",
  "fill-success stroke-success",
  "fill-attention stroke-attention",
  "fill-danger stroke-danger",
];

/** SVG geometry comes from data; a table provides the same values for assistive technology. */
export function AssistantChart({
  label,
  labels,
  series,
  kind = "line",
  stacked = false,
  locale = "en",
  visibleCount = labels.length,
  delta,
  trend,
}: ChartProps) {
  const titleId = useId();
  const { baseline, minimum, maximum, y, x, categoryWidth } = chartGeometry(
    labels,
    series,
    stacked,
    kind === "bar",
  );
  const point = (value: number, index: number) => `${x(index)},${y(value)}`;
  return (
    <AssistantPanel>
      <PanelHeading>{label}</PanelHeading>
      {delta === undefined ? null : (
        <p className="flex items-center gap-1 text-ui" data-trend={trend}>
          {trend === "up" || trend === "down" ? (
            <Icon name="arrow" className={trend === "up" ? "-rotate-90" : "rotate-90"} />
          ) : null}
          {delta}
        </p>
      )}
      <svg
        viewBox="0 0 600 250" // oxlint-disable-next-line jsx-a11y/prefer-tag-over-role -- Inline SVG is a titled chart with an equivalent data table.
        role="img"
        aria-labelledby={titleId}
        className="w-full"
      >
        <title id={titleId}>{label}</title>
        <path d={`M40 20V200 M40 ${y(0)}H570`} fill="none" className="stroke-control-border" />
        {[...new Set([minimum, 0, (minimum + maximum) / 2, maximum])].map((tick) => (
          <text
            key={tick}
            x={34}
            y={y(tick) + 5}
            textAnchor="end"
            className="fill-text-muted text-xs"
          >
            {new Intl.NumberFormat(locale, { notation: "compact" }).format(tick)}
          </text>
        ))}
        {series.map((entry, seriesIndex) => (
          <g key={entry.id} className={seriesClasses[seriesIndex % seriesClasses.length]}>
            {kind === "bar" ? (
              entry.values.slice(0, Math.max(0, visibleCount)).map((value, index) => (
                <rect
                  key={`${entry.id}-${labels[index]}`}
                  x={
                    x(index) -
                    categoryWidth * 0.4 +
                    (stacked ? 0 : (seriesIndex * categoryWidth * 0.8) / Math.max(1, series.length))
                  }
                  y={Math.min(
                    y(baseline(seriesIndex, index)),
                    y(value + baseline(seriesIndex, index)),
                  )}
                  width={(categoryWidth * 0.8) / (stacked ? 1 : Math.max(1, series.length))}
                  height={Math.abs(
                    y(value + baseline(seriesIndex, index)) - y(baseline(seriesIndex, index)),
                  )}
                  className="animate-assistant-fade-in motion-reduce:animate-none"
                >
                  <title>{`${labels[index]}: ${entry.label} ${value}`}</title>
                </rect>
              ))
            ) : (
              <>
                <polyline
                  points={entry.values
                    .slice(0, Math.max(0, visibleCount))
                    .map((value, index) => point(value + baseline(seriesIndex, index), index))
                    .join(" ")}
                  fill="none"
                  strokeWidth="2"
                />
                {kind === "area" ? (
                  <polygon
                    points={`${entry.values
                      .slice(0, Math.max(0, visibleCount))
                      .map((value, index) => point(value + baseline(seriesIndex, index), index))
                      .join(" ")} ${entry.values
                      .slice(0, Math.max(0, visibleCount))
                      .map((_, index) => point(baseline(seriesIndex, index), index))
                      .toReversed()
                      .join(" ")}`}
                    stroke="none"
                    className="opacity-20"
                  />
                ) : null}
              </>
            )}
          </g>
        ))}
        {labels.map((entry, index) => (
          <text
            key={entry}
            x={x(index)}
            y={230}
            textAnchor="middle"
            className="fill-text-muted text-xs"
          >
            {entry}
          </text>
        ))}
      </svg>
      <ul className="flex flex-wrap gap-3 text-xs">
        {series.map((entry, index) => (
          <li key={entry.id} className="flex items-center gap-2">
            <svg
              viewBox="0 0 8 8"
              aria-hidden="true"
              className={`size-2 ${seriesClasses[index % seriesClasses.length]}`}
            >
              <circle cx="4" cy="4" r="4" />
            </svg>
            {entry.label}
          </li>
        ))}
      </ul>
      <details className="mt-3 text-ui">
        <summary className="min-h-control cursor-pointer rounded-pill px-3 py-2 text-ui leading-ui font-medium text-primary hover:bg-surface-hover focus-visible:outline-2 focus-visible:outline-primary pointer-coarse:min-h-control-touch">
          {label}
        </summary>
        <div className="overflow-x-auto">
          <table className="w-full">
            <thead>
              <tr>
                <th scope="col">{label}</th>
                {series.map((entry) => (
                  <th key={entry.id} scope="col">
                    {entry.label}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {labels.map((entry, index) => (
                <tr key={entry}>
                  <th scope="row" className="text-left font-normal">
                    {entry}
                  </th>
                  {series.map((item) => (
                    <td key={item.id} className="text-right tabular-nums">
                      {item.values[index] ?? "—"}
                    </td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </details>
    </AssistantPanel>
  );
}

export function MediaPlayer({
  kind,
  src,
  label,
  captions,
  poster,
  errorLabel,
  onError,
}: {
  kind: "audio" | "video";
  src: string;
  label: string;
  captions?: string;
  poster?: string;
  errorLabel?: string;
  onError?: () => void;
}) {
  const [failedSource, setFailedSource] = useState<string>();
  const mediaError = () => {
    setFailedSource(src);
    onError?.();
  };
  const error =
    failedSource === src ? (
      <p role="alert" className="text-ui text-danger">
        {errorLabel ?? label}
      </p>
    ) : null;
  if (kind === "audio") {
    return (
      <section aria-label={label} className="space-y-3">
        {poster === undefined ? null : (
          <img src={poster} alt="" className="size-14 rounded-control object-cover" />
        )}
        <h3 className="text-ui font-medium">{label}</h3>
        {error}
        <audio
          onError={mediaError}
          controls
          preload="metadata"
          aria-label={label}
          src={src}
          className="w-full"
        >
          <track kind="captions" src={captions} />
        </audio>
      </section>
    );
  }
  return (
    <section aria-label={label} className="space-y-3">
      {error}
      <video
        onError={mediaError}
        controls
        preload="metadata"
        aria-label={label}
        src={src}
        poster={poster}
        className="min-h-20 w-full rounded-panel border-transparent"
      >
        <track kind="captions" src={captions} />
      </video>
    </section>
  );
}

/** Always sandbox generated pages. Hosts explicitly select a preview source. */
export function WebPreview({
  src,
  label,
  reloadLabel,
  openLabel,
  loadingLabel,
}: {
  src: string;
  label: string;
  reloadLabel: string;
  openLabel: string;
  loadingLabel?: string;
}) {
  const [revision, setRevision] = useState(0);
  const [loaded, setLoaded] = useState<string>();
  const revisionKey = `${src}:${revision}`;
  const loading = loaded !== revisionKey;
  return (
    <AssistantPanel>
      <PanelHeading
        actions={
          <div className="flex flex-wrap gap-2">
            <Button
              variant="ghost"
              onClick={() => {
                setRevision((value) => value + 1);
              }}
            >
              {reloadLabel}
            </Button>
            <Button asChild variant="secondary">
              <a href={src} target="_blank" rel="noreferrer">
                {openLabel}
              </a>
            </Button>
          </div>
        }
      >
        {label}
      </PanelHeading>
      <p className="truncate text-xs text-text-muted">{src}</p>
      {loading ? (
        <output className="flex items-center gap-2 text-ui">
          <Icon name="loading" className="animate-spin motion-reduce:animate-none" />
          {loadingLabel ?? label}
        </output>
      ) : null}
      <iframe
        key={revision}
        src={src}
        title={label}
        sandbox=""
        referrerPolicy="no-referrer"
        onLoad={() => {
          setLoaded(revisionKey);
        }}
        aria-busy={loading}
        className={cn(
          "h-80 w-full rounded-control border border-transparent bg-surface transition-opacity duration-300 motion-reduce:transition-none",
          loading && "opacity-0",
        )}
      />
    </AssistantPanel>
  );
}

export function Diagram({
  label,
  children,
  zoomInLabel,
  zoomOutLabel,
  resetLabel,
  fullscreenLabel,
  closeLabel,
}: {
  label: string;
  children: ReactNode;
  zoomInLabel: string;
  zoomOutLabel: string;
  resetLabel: string;
  fullscreenLabel: string;
  closeLabel: string;
}) {
  const [zoom, setZoom] = useState(1);
  const [open, setOpen] = useState(false);
  const scales = ["scale-100", "scale-125", "scale-150"];
  const diagram = (
    <div
      className={cn(
        "origin-top-left transition-transform duration-200 ease-out motion-reduce:transition-none",
        scales[zoom - 1],
      )}
    >
      {children}
    </div>
  );
  return (
    <AssistantPanel>
      <PanelHeading
        actions={
          <div className="flex flex-wrap gap-2">
            <Button
              variant="ghost"
              disabled={zoom === 1}
              onClick={() => {
                setZoom((value) => Math.max(1, value - 1));
              }}
            >
              {zoomOutLabel}
            </Button>
            <Button
              variant="ghost"
              disabled={zoom === 3}
              onClick={() => {
                setZoom((value) => Math.min(3, value + 1));
              }}
            >
              {zoomInLabel}
            </Button>
            <Button
              variant="secondary"
              onClick={() => {
                setZoom(1);
              }}
            >
              {resetLabel}
            </Button>
            <Button
              variant="ghost"
              onClick={() => {
                setOpen(true);
              }}
            >
              {fullscreenLabel}
            </Button>
          </div>
        }
      >
        {label}
      </PanelHeading>
      <output className="text-xs tabular-nums">{100 + (zoom - 1) * 25}%</output>
      <div className="overflow-auto">{diagram}</div>
      <AssistantModal open={open} onOpenChange={setOpen} title={label} closeLabel={closeLabel}>
        <div className="overflow-auto">{diagram}</div>
      </AssistantModal>
    </AssistantPanel>
  );
}

export type FlowNode = {
  id: string;
  label: string;
  x: number;
  y: number;
  state: ExecutionState;
  statusLabel: string;
};
export type FlowEdge = { id: string; from: string; to: string };

export function FlowGraph({
  label,
  nodes,
  edges,
  onSelect,
}: {
  label: string;
  nodes: readonly FlowNode[];
  edges: readonly FlowEdge[];
  onSelect: (id: string) => void;
}) {
  return (
    <AssistantPanel>
      <PanelHeading>{label}</PanelHeading>
      <svg
        viewBox="0 0 600 260"
        aria-label={label} // oxlint-disable-next-line jsx-a11y/prefer-tag-over-role -- Inline SVG is a titled graph with an equivalent node list.
        role="img"
        className="w-full"
      >
        <title>{label}</title>
        {edges.map((edge) => {
          const from = nodes.find((node) => node.id === edge.from);
          const to = nodes.find((node) => node.id === edge.to);
          return from && to ? (
            <path
              key={edge.id}
              d={`M${from.x + 60} ${from.y + 20} L${to.x + 60} ${to.y + 20}`}
              fill="none"
              className="animate-assistant-edge-in stroke-control-border motion-reduce:animate-none"
              strokeWidth="2"
            />
          ) : null;
        })}
        {nodes.map((node) => (
          <g key={node.id} className="animate-assistant-node-in motion-reduce:animate-none">
            <title>{`${node.label}: ${node.statusLabel}`}</title>
            <rect
              x={node.x}
              y={node.y}
              width="120"
              height="40"
              rx="8"
              className={cn(
                "transition-colors duration-300 motion-reduce:transition-none",
                node.state === "running" && "fill-primary-soft stroke-primary",
                node.state === "completed" && "fill-success/20 stroke-success",
                node.state === "failed" && "fill-danger/20 stroke-danger",
                node.state !== "running" &&
                  node.state !== "completed" &&
                  node.state !== "failed" &&
                  "fill-surface-muted stroke-control-border",
              )}
            />
            <text x={node.x + 60} y={node.y + 25} textAnchor="middle" className="fill-ink text-ui">
              {node.label}
            </text>
          </g>
        ))}
      </svg>
      <ol className="flex flex-wrap gap-2">
        {nodes.map((node) => (
          <li key={node.id}>
            <Button
              variant="secondary"
              onClick={() => {
                onSelect(node.id);
              }}
            >
              {node.label}
              <ExecutionStatus state={node.state}>{node.statusLabel}</ExecutionStatus>
            </Button>
          </li>
        ))}
      </ol>
    </AssistantPanel>
  );
}

export type ActivityDay = { date: string; value: number; label: string };
const heatClasses = [
  "bg-surface-muted",
  "bg-primary-soft",
  "bg-primary/40",
  "bg-primary/70",
  "bg-primary",
];

export function ActivityGraph({
  days,
  label,
  legend,
  locale = "en",
  totalLabel,
}: {
  days: readonly ActivityDay[];
  label: string;
  legend: string;
  locale?: string;
  totalLabel?: string;
}) {
  const dated = days
    .filter((day) => /^\d{4}-\d{2}-\d{2}$/u.test(day.date) && Number.isFinite(Date.parse(day.date)))
    .toSorted((a, b) => a.date.localeCompare(b.date));
  const [first] = dated;
  const last = dated.at(-1);
  const firstTime = first ? Date.parse(first.date) : 0;
  const offset = first ? (new Date(firstTime).getUTCDay() + 6) % 7 : 0;
  const byDate = new Map(dated.map((day) => [day.date, day]));
  const length =
    first && last ? Math.round((Date.parse(last.date) - firstTime) / 86_400_000) + 1 : 0;
  const cells = Array.from({ length: Math.min(3660, length + offset) }, (_, index) => {
    const date = new Date(firstTime + (index - offset) * 86_400_000);
    const key = date.toISOString().slice(0, 10);
    return { date, key, day: byDate.get(key), blank: index < offset };
  });
  return (
    <AssistantPanel>
      <PanelHeading>{label}</PanelHeading>
      <div className="flex gap-2 overflow-x-auto">
        <div className="mt-5 grid grid-rows-7 gap-1 text-xs text-text-muted">
          {Array.from({ length: 7 }, (_, index) => (
            <span key={index + 1} className="h-3 leading-3">
              {new Intl.DateTimeFormat(locale, { weekday: "short", timeZone: "UTC" }).format(
                new Date(Date.UTC(2026, 0, 5 + index)),
              )}
            </span>
          ))}
        </div>
        <div>
          <div className="mb-1 flex h-4 text-xs text-text-muted">
            {cells
              .filter((_, index) => index % 7 === 0)
              .map((cell, index, weeks) => (
                <span key={cell.key} className="w-4 shrink-0 overflow-visible whitespace-nowrap">
                  {index === 0 || cell.date.getUTCMonth() !== weeks[index - 1]?.date.getUTCMonth()
                    ? new Intl.DateTimeFormat(locale, { month: "short", timeZone: "UTC" }).format(
                        cell.date,
                      )
                    : ""}
                </span>
              ))}
          </div>
          <div className="grid w-max grid-flow-col grid-rows-7 gap-1">
            {cells.map((cell) => (
              <span key={cell.key} className="group relative">
                <span
                  data-placeholder={cell.blank || undefined}
                  tabIndex={cell.blank ? undefined : 0}
                  aria-label={cell.day?.label ?? cell.key}
                  className={cn(
                    "block size-3 rounded-pill focus-visible:outline-2 focus-visible:outline-primary",
                    cell.blank
                      ? "invisible"
                      : heatClasses[Math.min(4, Math.max(0, Math.floor(cell.day?.value ?? 0)))],
                  )}
                />
                <span
                  role="tooltip"
                  className="pointer-events-none invisible absolute bottom-full left-0 z-10 w-max rounded-control bg-inverse px-2 py-1 text-xs text-inverse-text group-focus-within:visible group-hover:visible"
                >
                  {cell.day?.label ?? cell.key}
                </span>
              </span>
            ))}
          </div>
        </div>
      </div>
      <p className="mt-3 text-xs text-text-muted">
        {totalLabel === undefined
          ? ""
          : `${totalLabel}: ${days.reduce((sum, day) => sum + day.value, 0)} · `}
        {first?.date} – {last?.date}
      </p>
      <div className="mt-2 flex items-center gap-1">
        <span className="mr-2 text-xs text-text-muted">{legend}</span>
        {heatClasses.map((color) => (
          <span key={color} className={cn("size-3 rounded-pill", color)} aria-hidden="true" />
        ))}
      </div>
    </AssistantPanel>
  );
}

export function SpecSheet({
  title,
  subtitle,
  items,
  visibleCount = items.length,
}: {
  title: string;
  subtitle?: string;
  items: readonly (LabeledValue & { emphasis?: boolean })[];
  visibleCount?: number;
}) {
  return (
    <AssistantPanel>
      <PanelHeading>{title}</PanelHeading>
      {subtitle === undefined ? null : <p className="mb-3 text-ui text-text-muted">{subtitle}</p>}
      <dl className="space-y-2">
        {items.slice(0, Math.max(0, visibleCount)).map((item) => (
          <div
            key={item.label}
            className="flex animate-assistant-fade-in justify-between gap-3 text-ui motion-reduce:animate-none"
          >
            <dt className="text-text-muted">{item.label}</dt>
            <dd className={cn("text-right", item.emphasis === true && "font-semibold")}>
              {item.value}
            </dd>
          </div>
        ))}
      </dl>
    </AssistantPanel>
  );
}

export type TimelineEvent = {
  id: string;
  title: string;
  detail: string;
  time: string;
  dateTime?: string;
  when?: "past" | "now" | "future";
  state: ExecutionState;
  statusLabel: string;
};

export function Timeline({
  events,
  label,
  visibleCount = events.length,
}: {
  events: readonly TimelineEvent[];
  label: string;
  visibleCount?: number;
}) {
  return (
    <ol aria-label={label} className="space-y-4">
      {events.slice(0, Math.max(0, visibleCount)).map((event) => (
        <li
          key={event.id}
          className="grid animate-assistant-slide-left grid-cols-[auto_1fr] gap-3 motion-reduce:animate-none"
          data-when={event.when}
        >
          <span className="flex w-4 flex-col items-center" aria-hidden="true">
            <span
              className={cn(
                "mt-1 size-2 rounded-full",
                event.when === "now" ? "bg-primary ring-4 ring-primary/20" : "bg-text-muted",
                event.when === "future" && "opacity-30",
              )}
            />
            <span className="mt-1 w-px flex-1 bg-control-border" />
          </span>
          <div className="space-y-1">
            <div className="flex flex-wrap items-center justify-between gap-2">
              <h4 className="text-ui font-medium">{event.title}</h4>
              <ExecutionStatus state={event.state}>{event.statusLabel}</ExecutionStatus>
            </div>
            <p className="text-ui text-text-muted">{event.detail}</p>
            <time dateTime={event.dateTime} className="text-xs text-text-muted tabular-nums">
              {event.time}
            </time>
          </div>
        </li>
      ))}
    </ol>
  );
}

export type JobStage = { id: string; label: string; weight: number; progress: number };

export function JobProgress({
  label,
  stages,
  eta,
  cancelLabel,
  onCancel,
  outcome,
  elapsed,
}: {
  label: string;
  stages: readonly JobStage[];
  eta: string;
  cancelLabel: string;
  onCancel: () => void;
  outcome?: { state: "completed" | "failed" | "cancelled"; label: string; summary: string };
  elapsed?: string;
}) {
  const total = stages.reduce((sum, stage) => sum + Math.max(0, stage.weight), 0);
  const completed = stages.reduce(
    (sum, stage) => sum + Math.max(0, stage.weight) * Math.min(100, Math.max(0, stage.progress)),
    0,
  );
  return (
    <AssistantPanel>
      <PanelHeading
        actions={
          outcome ? undefined : (
            <Badge>
              <Icon name="loading" className="animate-spin motion-reduce:animate-none" />
              {eta}
            </Badge>
          )
        }
      >
        {label}
      </PanelHeading>
      <AssistantProgress label={label} value={total > 0 ? completed / total : 0} />
      <ul className="my-4 space-y-3">
        {stages.map((stage) => (
          <li key={stage.id} className="space-y-1">
            <p className="text-ui">{stage.label}</p>
            <AssistantProgress label={stage.label} value={stage.progress} />
          </li>
        ))}
      </ul>
      {outcome ? (
        <output className="block space-y-2">
          <ExecutionStatus state={outcome.state}>{outcome.label}</ExecutionStatus>
          <p className="text-ui">{outcome.summary}</p>
          {elapsed === undefined ? null : <p className="text-xs text-text-muted">{elapsed}</p>}
        </output>
      ) : (
        <Button variant="secondary" onClick={onCancel}>
          {cancelLabel}
        </Button>
      )}
    </AssistantPanel>
  );
}

export type ScoreCriterion = {
  id: string;
  label: string;
  score: number;
  maximum: number;
  weight: number;
  explanation: string;
};

export function ScoreBreakdown({
  label,
  criteria,
  total,
  outOf,
  verdict,
  visibleCount = criteria.length,
}: {
  label: string;
  criteria: readonly ScoreCriterion[];
  total?: number;
  outOf?: number;
  verdict?: string;
  visibleCount?: number;
}) {
  return (
    <AssistantPanel>
      <PanelHeading>{label}</PanelHeading>
      {total === undefined ? null : (
        <p className="text-title tabular-nums">
          {total} / {outOf} <span className="text-ui">{verdict}</span>
        </p>
      )}
      <ul className="space-y-3">
        {criteria.slice(0, Math.max(0, visibleCount)).map((criterion) => (
          <li key={criterion.id} className="animate-assistant-slide-up motion-reduce:animate-none">
            <div className="flex flex-wrap items-center justify-between gap-2 text-ui">
              <span className="font-medium">{criterion.label}</span>
              <span className="tabular-nums">
                {criterion.score} / {criterion.maximum} · {criterion.weight}%
              </span>
            </div>
            <AssistantProgress
              label={criterion.label}
              value={criterion.score}
              max={criterion.maximum}
            />
            <p className="text-xs text-text-muted">{criterion.explanation}</p>
          </li>
        ))}
      </ul>
    </AssistantPanel>
  );
}
