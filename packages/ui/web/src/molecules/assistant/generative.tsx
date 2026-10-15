import type { ReactNode } from "react";

import { Badge } from "../../atoms/badge";
import { Button } from "../../atoms/button";
import { Icon } from "../../atoms/icon";
import type { IconName } from "../../atoms/icon";
import { Separator } from "../../atoms/separator";
import { cn } from "../../classes";
import { MarkdownText } from "./renderers";
import { AssistantPanel, AssistantProgress, Facts } from "./shared";
import { AssistantChart, DataTable } from "./structured";
import type { ChartProps } from "./structured";
import { ElicitationForm } from "./tools";
import type { ElicitationField } from "./tools";

export type GenerativeAction = {
  id: string;
  label: string;
  variant?: "primary" | "secondary" | "ghost" | "destructive";
  disabled?: boolean;
};

/** Swarmie-owned structured presentation. Validate external payloads before creating this tree. */
export type GenerativeNode =
  | { id: string; kind: "card"; title: string; children: readonly GenerativeNode[] }
  | { id: string; kind: "row" | "column" | "box"; children: readonly GenerativeNode[] }
  | { id: string; kind: "spacer" }
  | { id: string; kind: "icon"; icon: IconName; label: string }
  | { id: string; kind: "progress"; value: number; label: string }
  | { id: string; kind: "text"; text: string; emphasis?: "normal" | "title" | "muted" }
  | { id: string; kind: "markdown"; text: string }
  | {
      id: string;
      kind: "badge";
      text: string;
      tone?: "neutral" | "agent" | "success" | "attention" | "danger";
    }
  | { id: string; kind: "image"; src: string; alt: string; caption?: string }
  | { id: string; kind: "facts"; items: readonly { label: string; value: string }[] }
  | {
      id: string;
      kind: "list";
      items: readonly {
        id: string;
        title: string;
        detail: string;
        action?: GenerativeAction;
        image?: { src: string; alt: string; round?: boolean };
        marker?: boolean;
        badge?: string;
      }[];
    }
  | { id: string; kind: "actions"; actions: readonly GenerativeAction[] }
  | { id: string; kind: "divider" }
  | { id: string; kind: "chart"; chart: ChartProps }
  | {
      id: string;
      kind: "table";
      title: string;
      columns: readonly { id: string; label: string }[];
      rows: readonly { id: string; values: Readonly<Record<string, string>> }[];
      emptyLabel: string;
    }
  | {
      id: string;
      kind: "form";
      title: string;
      description: string;
      fields: readonly ElicitationField[];
      submit: GenerativeAction;
      cancel: GenerativeAction;
      summary?: string;
    };

const emptyValues: Readonly<Record<string, string>> = {};

export type GenerativeUIProps = {
  node: GenerativeNode;
  values?: Readonly<Record<string, string>>;
  onFieldChange: (id: string, value: string) => void;
  onAction: (id: string, values?: Readonly<Record<string, string>>) => void;
};

/** Structured output is rendered through existing components; arbitrary tags and handlers are excluded. */
export function GenerativeUI({
  node,
  values = emptyValues,
  onFieldChange,
  onAction,
}: GenerativeUIProps): ReactNode {
  const children = (nodes: readonly GenerativeNode[]) =>
    nodes.map((child) => (
      <GenerativeUI
        key={child.id}
        node={child}
        values={values}
        onFieldChange={onFieldChange}
        onAction={onAction}
      />
    ));
  const action = (item: GenerativeAction) => (
    <Button
      key={item.id}
      variant={item.variant ?? "primary"}
      disabled={item.disabled}
      onClick={() => {
        onAction(item.id);
      }}
    >
      {item.label}
    </Button>
  );
  switch (node.kind) {
    case "card": {
      return (
        <AssistantPanel>
          <h3 className="mb-4 text-title">{node.title}</h3>
          <div className="space-y-3">{children(node.children)}</div>
        </AssistantPanel>
      );
    }
    case "spacer": {
      return <div className="h-3" aria-hidden="true" />;
    }
    case "icon": {
      return (
        <span className="inline-flex items-center gap-2 text-ui">
          <Icon name={node.icon} />
          {node.label}
        </span>
      );
    }
    case "progress": {
      return <AssistantProgress label={node.label} value={node.value} />;
    }
    case "column": {
      return <div className="flex min-w-0 flex-1 flex-col gap-2">{children(node.children)}</div>;
    }
    case "box": {
      return <div className="rounded-control bg-surface-muted p-3">{children(node.children)}</div>;
    }
    case "row": {
      return <div className="flex flex-wrap items-start gap-3">{children(node.children)}</div>;
    }
    case "text": {
      return <GenerativeText node={node} />;
    }
    case "markdown": {
      return <MarkdownText text={node.text} />;
    }
    case "badge": {
      return <GenerativeBadge node={node} />;
    }
    case "image": {
      return <GenerativeImage node={node} />;
    }
    case "facts": {
      return <Facts items={node.items} />;
    }
    case "list": {
      return (
        <ul className="space-y-3">
          {node.items.map((item) => {
            const content = (
              <>
                {item.marker === true ? (
                  <span aria-hidden="true" className="h-8 w-1 shrink-0 rounded-pill bg-primary" />
                ) : null}
                {item.image ? (
                  <img
                    src={item.image.src}
                    alt={item.image.alt}
                    className={cn(
                      "size-10 object-cover",
                      item.image.round === true ? "rounded-full" : "rounded-control",
                    )}
                  />
                ) : null}
                <span className="min-w-0 flex-1">
                  <span className="block text-ui font-medium">{item.title}</span>
                  <span className="block text-xs text-text-muted">{item.detail}</span>
                </span>
                {item.badge === undefined ? null : <Badge>{item.badge}</Badge>}
                {item.action ? (
                  <span className="text-xs text-primary">{item.action.label}</span>
                ) : null}
              </>
            );
            return (
              <li key={item.id}>
                {item.action ? (
                  <Button
                    variant="ghost"
                    className="h-auto w-full justify-start rounded-control p-2 text-left"
                    disabled={item.action.disabled}
                    onClick={() => {
                      if (item.action) {
                        onAction(item.action.id);
                      }
                    }}
                  >
                    {content}
                  </Button>
                ) : (
                  <div className="flex items-center gap-3">{content}</div>
                )}
              </li>
            );
          })}
        </ul>
      );
    }
    case "actions": {
      return <div className="flex flex-wrap gap-2">{node.actions.map(action)}</div>;
    }
    case "divider": {
      return <Separator />;
    }
    case "chart": {
      return <AssistantChart {...node.chart} />;
    }
    case "table": {
      return (
        <DataTable<{ id: string; values: Readonly<Record<string, string>> }>
          caption={node.title}
          emptyLabel={node.emptyLabel}
          rowId={(row) => row.id}
          rows={node.rows}
          columns={node.columns.map((column) => ({
            ...column,
            render: (row) => row.values[column.id] ?? "",
          }))}
        />
      );
    }
    case "form": {
      return (
        <GenerativeForm
          node={node}
          values={values}
          onFieldChange={onFieldChange}
          onAction={onAction}
        />
      );
    }
    default: {
      return assertNever(node);
    }
  }
}

function assertNever(node: never): never {
  throw new Error(`Unsupported generative node: ${String(node)}`);
}

function GenerativeText({ node }: { node: Extract<GenerativeNode, { kind: "text" }> }) {
  return (
    <p
      className={cn(
        "text-content",
        node.emphasis === "title" && "text-title",
        node.emphasis === "muted" && "text-ui text-text-muted",
      )}
    >
      {node.text}
    </p>
  );
}

function GenerativeImage({ node }: { node: Extract<GenerativeNode, { kind: "image" }> }) {
  return (
    <figure>
      <img
        src={node.src}
        alt={node.alt}
        loading="lazy"
        className="aspect-video w-full rounded-control object-cover"
      />
      {node.caption === undefined ? null : (
        <figcaption className="mt-2 text-xs text-text-muted">{node.caption}</figcaption>
      )}
    </figure>
  );
}

function GenerativeForm({
  node,
  values = emptyValues,
  onFieldChange,
  onAction,
}: Omit<GenerativeUIProps, "node"> & { node: Extract<GenerativeNode, { kind: "form" }> }) {
  return (
    <div className="space-y-3">
      {node.summary === undefined ? null : <p className="text-title">{node.summary}</p>}
      <ElicitationForm
        title={node.title}
        description={node.description}
        fields={node.fields.map((field) => ({
          ...field,
          value: values[field.id] ?? field.value,
        }))}
        onChange={onFieldChange}
        submitDisabled={node.submit.disabled ?? false}
        cancelDisabled={node.cancel.disabled ?? false}
        submitLabel={node.submit.label}
        cancelLabel={node.cancel.label}
        onSubmit={(submittedValues) => {
          onAction(node.submit.id, submittedValues);
        }}
        onCancel={() => {
          onAction(node.cancel.id);
        }}
      />
    </div>
  );
}

function GenerativeBadge({ node }: { node: Extract<GenerativeNode, { kind: "badge" }> }) {
  return <Badge tone={node.tone ?? "neutral"}>{node.text}</Badge>;
}
