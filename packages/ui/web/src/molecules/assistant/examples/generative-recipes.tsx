import { useState } from "react";

import type { AssistantElementSlugFor } from "../catalog";
import { GenerativeUI } from "../generative";
import type { GenerativeNode } from "../generative";
import { chartLabels, chartSeries, demoImage, ExampleReceipt, useExampleAction } from "./fixtures";

function facts(items: readonly { label: string; value: string }[]): GenerativeNode {
  return { id: "facts", kind: "facts", items };
}

function actions(primary: string, secondary = "Cancel"): GenerativeNode {
  return {
    id: "actions",
    kind: "actions",
    actions: [
      { id: "confirm", label: primary },
      { id: "cancel", label: secondary, variant: "secondary" },
    ],
  };
}

function card(title: string, children: readonly GenerativeNode[]): GenerativeNode {
  return { id: "card", kind: "card", title, children };
}

/** Every recipe uses the same public structured UI contract and real controlled form values. */
type GenerativeSlug = AssistantElementSlugFor<"Generative">;

export function generativeRecipe(slug: GenerativeSlug): GenerativeNode {
  const recipes: Record<GenerativeSlug, GenerativeNode> = {
    "generative-stays": card("Riverside Loft", [
      {
        id: "image",
        kind: "image",
        src: demoImage,
        alt: "Illustrative accommodation fixture",
        caption: "Demo accommodation · Replace with a verified property photo",
      },
      {
        id: "metadata",
        kind: "row",
        children: [
          { id: "rating", kind: "badge", text: "4.9 · Synthetic rating" },
          { id: "location", kind: "text", text: "Lisbon · Demo property", emphasis: "muted" },
        ],
      },
      facts([{ label: "Stay", value: "2 guests · 3 nights" }]),
      { id: "price", kind: "text", text: "$120 per night · $360 for 3 nights", emphasis: "title" },
      actions("Book stay", "Save for later"),
    ]),
    "generative-booking": {
      id: "reservation",
      kind: "form",
      title: "Reserve a table",
      description: "Choose the details for this demo reservation.",
      fields: [
        { id: "name", label: "Guest name", kind: "text", value: "", required: true },
        {
          id: "party",
          label: "Party size",
          kind: "choice",
          value: "2",
          options: [
            { value: "2", label: "2 people" },
            { value: "4", label: "4 people" },
            { value: "6", label: "6 people" },
          ],
        },
        { id: "date", label: "Date", kind: "date", value: "2026-10-03" },
        {
          id: "seating",
          label: "Seating",
          kind: "radio",
          value: "inside",
          options: [
            { value: "inside", label: "Inside" },
            { value: "terrace", label: "Terrace" },
          ],
        },
        {
          id: "reminder",
          label: "Send a reminder 1 hour before",
          kind: "checkbox",
          value: "false",
        },
      ],
      submit: { id: "book", label: "Confirm reservation" },
      cancel: { id: "cancel", label: "Cancel" },
    },
    "generative-order-status": card("Order #DEMO-1842", [
      { id: "status", kind: "badge", text: "Out for delivery", tone: "agent" },
      {
        id: "notice",
        kind: "box",
        children: [
          {
            id: "confirmation",
            kind: "icon",
            icon: "circle-check",
            label: "Order confirmed · Your order is on its way",
          },
        ],
      },
      {
        id: "steps",
        kind: "list",
        items: [
          {
            id: "placed",
            title: "Order placed",
            detail: "September 29",
            badge: "Complete",
            action: { id: "view-placed", label: "View event" },
          },
          {
            id: "shipped",
            title: "Shipped",
            detail: "September 30",
            badge: "Complete",
            action: { id: "view-shipped", label: "View event" },
          },
          {
            id: "arrival",
            title: "Delivery",
            detail: "Today · 14:00–18:00",
            badge: "In progress",
            action: { id: "view-arrival", label: "View event" },
          },
        ],
      },
      actions("Track package", "Contact support"),
    ]),
    "generative-flight-tracker": card("Flight SW 204 · Demo", [
      { id: "route", kind: "text", text: "LIS → AMS", emphasis: "title" },
      { id: "status", kind: "badge", text: "In the air", tone: "agent" },
      facts([
        { label: "Departed", value: "Lisbon · 10:20" },
        { label: "Arriving", value: "Amsterdam · 14:25" },
        { label: "Progress", value: "1 h 40 m of 3 h 05 m" },
      ]),
      { id: "flight-progress", kind: "progress", value: 62, label: "Route completed: 62%" },
      { id: "aircraft", kind: "icon", icon: "plane", label: "En route to Amsterdam" },
      actions("View flight details", "Save flight"),
    ]),
    "generative-portfolio": card("Portfolio overview · Demo", [
      facts([
        { label: "Total value", value: "$42,800.00" },
        { label: "Today", value: "+$318.40 (+0.75%)" },
        { label: "Positions", value: "2" },
      ]),
      {
        id: "chart",
        kind: "chart",
        chart: {
          label: "Demo portfolio performance",
          labels: ["July", "August", "September"],
          series: [{ id: "value", label: "Value", values: [38_000, 40_800, 42_800] }],
          kind: "line",
          stacked: true,
        },
      },
      {
        id: "positions",
        kind: "table",
        title: "Demo positions",
        columns: [
          { id: "asset", label: "Asset" },
          { id: "shares", label: "Shares" },
          { id: "price", label: "Price" },
          { id: "value", label: "Value" },
        ],
        rows: [
          {
            id: "funds",
            values: { asset: "Broad fund", shares: "120", price: "$200", value: "$24,000" },
          },
          {
            id: "stocks",
            values: { asset: "Demo equities", shares: "100", price: "$148", value: "$14,800" },
          },
        ],
        emptyLabel: "No positions",
      },
      {
        id: "disclaimer",
        kind: "markdown",
        text: "**Illustrative data.** Includes $4,000 in cash. Past performance does not predict future returns.",
      },
    ]),
    "generative-create-event": card("Add a design review", [
      {
        id: "date-tile",
        kind: "box",
        children: [{ id: "date", kind: "icon", icon: "calendar", label: "October 3 · 2026" }],
      },
      { id: "intro", kind: "text", text: "October 3 · 14:00–14:45 · Demo calendar" },
      {
        id: "events",
        kind: "list",
        items: [
          { id: "standup", title: "Team check-in", detail: "09:00–09:15", badge: "Work" },
          { id: "focus", title: "Focus time", detail: "10:00–12:00", badge: "Focus" },
          { id: "review", title: "Design review", detail: "14:00–14:45 · Proposed", badge: "Work" },
        ],
      },
      actions("Add to calendar", "Discard"),
    ]),
    "generative-view-event": card("Design system review", [
      facts([
        { label: "When", value: "October 3 · 14:00–14:45" },
        { label: "Where", value: "Shared workspace · Demo room" },
        { label: "People", value: "Alex, Maya, and Swarmie" },
      ]),
      actions("View event", "Dismiss"),
    ]),
    "generative-weather-current": card("Lisbon weather · Demo", [
      { id: "weather-symbol", kind: "icon", icon: "cloud", label: "Partly cloudy" },
      { id: "temperature", kind: "text", text: "22°C", emphasis: "title" },
      { id: "conditions", kind: "text", text: "Partly cloudy · Synthetic weather data" },
      facts([
        { label: "Feels like", value: "23°C" },
        { label: "Wind", value: "12 km/h NW" },
        { label: "Humidity", value: "64%" },
      ]),
    ]),
    "generative-ride-status": card("Your demo ride is arriving", [
      { id: "eta", kind: "text", text: "3 minutes", emphasis: "title" },
      facts([
        { label: "Pickup", value: "Design studio · Demo location" },
        { label: "Driver", value: "Maya · Demo driver" },
        { label: "Vehicle", value: "Blue sedan · DEMO-42" },
      ]),
      actions("View pickup", "Cancel ride"),
    ]),
    "generative-draft-email": {
      id: "email",
      kind: "form",
      title: "Draft email",
      description: "Review this demo message before sending.",
      fields: [
        { id: "from", label: "From", kind: "text", value: "alex@example.com", readOnly: true },
        { id: "to", label: "To", kind: "text", value: "team@example.com", required: true },
        {
          id: "subject",
          label: "Subject",
          kind: "text",
          value: "Design system review",
          required: true,
        },
        {
          id: "body",
          label: "Message",
          kind: "multiline",
          value:
            "The shared components are ready for review. Let’s walk through the interaction states together.",
        },
      ],
      submit: { id: "send", label: "Send demo email" },
      cancel: { id: "discard", label: "Discard draft" },
    },
    "generative-cart": card("Shopping cart · Demo", [
      {
        id: "items",
        kind: "list",
        items: [
          { id: "notebook", title: "Studio notebook × 2", detail: "$24.00" },
          { id: "pen", title: "Drawing pen × 1", detail: "$8.00" },
        ],
      },
      { id: "divider", kind: "divider" },
      facts([
        { label: "Subtotal", value: "$32.00" },
        { label: "Tax", value: "$2.56" },
        { label: "Total", value: "$34.56" },
      ]),
      actions("Purchase", "Add items"),
    ]),
    "generative-playlist": card("Studio focus · Demo playlist", [
      { id: "cover", kind: "image", src: demoImage, alt: "Illustrative playlist cover" },
      {
        id: "tracks",
        kind: "list",
        items: [
          {
            id: "one",
            title: "1. First light",
            detail: "Demo artist · 3:42",
            image: { src: demoImage, alt: "Illustrative track artwork" },
            action: { id: "play-one", label: "Play", variant: "secondary" },
          },
          {
            id: "two",
            title: "2. Open workspace",
            detail: "Demo artist · 4:18",
            image: { src: demoImage, alt: "Illustrative track artwork" },
            action: { id: "play-two", label: "Play", variant: "secondary" },
          },
          {
            id: "three",
            title: "3. Quiet progress",
            detail: "Demo artist · 3:56",
            image: { src: demoImage, alt: "Illustrative track artwork" },
            action: { id: "play-three", label: "Play", variant: "secondary" },
          },
        ],
      },
      {
        id: "playlist-action",
        kind: "actions",
        actions: [{ id: "view-playlist", label: "View playlist", variant: "secondary" }],
      },
    ]),
    "generative-channel-message": card("#design · Today, 12:00", [
      {
        id: "author",
        kind: "list",
        items: [
          {
            id: "alex",
            title: "Alex Morgan",
            detail: "Design collaborator · Today, 12:00",
            image: { src: demoImage, alt: "Illustrative author avatar", round: true },
          },
        ],
      },
      {
        id: "body",
        kind: "markdown",
        text: "The component catalog is ready. Please review **keyboard behavior**, narrow layouts, and error states.\n\n- Composer and messages\n- Tool results and approvals\n- Structured output",
      },
      actions("Open channel", "Reply"),
    ]),
    "generative-receipt": card("Purchase receipt · Demo", [
      { id: "paid", kind: "icon", icon: "circle-check", label: "Paid" },
      {
        id: "product",
        kind: "list",
        items: [
          {
            id: "notebook",
            title: "Studio notebook × 2",
            detail: "Demo studio store",
            image: { src: demoImage, alt: "Illustrative notebook thumbnail" },
          },
        ],
      },
      facts([
        { label: "Item", value: "Studio notebook × 2" },
        { label: "Seller", value: "Demo studio store" },
        { label: "Delivery", value: "October 5–7" },
        { label: "Paid", value: "$24.00" },
      ]),
      actions("View receipt", "View order"),
    ]),
    "generative-chart-area": {
      id: "chart",
      kind: "chart",
      chart: {
        label: "Weekly active users · Demo",
        labels: chartLabels,
        series: chartSeries,
        kind: "area",
        stacked: true,
      },
    },
    "generative-player-card": card("Alex Morgan · Demo player", [
      { id: "team", kind: "text", text: "Studio team · Midfield" },
      {
        id: "stats",
        kind: "row",
        children: [
          {
            id: "games",
            kind: "column",
            children: [
              { id: "value", kind: "text", text: "24", emphasis: "title" },
              { id: "caption", kind: "text", text: "Games", emphasis: "muted" },
            ],
          },
          {
            id: "goals",
            kind: "column",
            children: [
              { id: "value", kind: "text", text: "8", emphasis: "title" },
              { id: "caption", kind: "text", text: "Goals", emphasis: "muted" },
            ],
          },
          {
            id: "assists",
            kind: "column",
            children: [
              { id: "value", kind: "text", text: "12", emphasis: "title" },
              { id: "caption", kind: "text", text: "Assists", emphasis: "muted" },
            ],
          },
          {
            id: "minutes",
            kind: "column",
            children: [
              { id: "value", kind: "text", text: "1,840", emphasis: "title" },
              { id: "caption", kind: "text", text: "Minutes", emphasis: "muted" },
            ],
          },
        ],
      },
    ]),
    "generative-event-session": card("Making agent work inspectable", [
      { id: "track", kind: "badge", text: "Design systems track" },
      {
        id: "description",
        kind: "text",
        text: "A demo conference session about clear contracts, shared context, and human decisions.",
      },
      facts([
        { label: "When", value: "October 12 · 10:00–10:45" },
        { label: "Room", value: "Studio A · Demo venue" },
      ]),
      {
        id: "speakers",
        kind: "list",
        items: [
          {
            id: "alex",
            title: "Alex Morgan",
            detail: "Design lead · Demo speaker",
            image: { src: demoImage, alt: "Illustrative Alex avatar", round: true },
            action: { id: "view-alex", label: "View", variant: "secondary" },
          },
          {
            id: "maya",
            title: "Maya Chen",
            detail: "Engineering lead · Demo speaker",
            image: { src: demoImage, alt: "Illustrative Maya avatar", round: true },
            action: { id: "view-maya", label: "View", variant: "secondary" },
          },
        ],
      },
    ]),
    "generative-notify-confirm": card("Apply the proposed change?", [
      {
        id: "description",
        kind: "text",
        text: "This demo updates the component contract and its examples.",
      },
      actions("Yes, apply change", "No, keep current version"),
    ]),
    "generative-chart-line": {
      id: "chart",
      kind: "chart",
      chart: {
        label: "Response time · Demo",
        labels: chartLabels,
        series: [{ id: "latency", label: "Milliseconds", values: [420, 380, 410, 340, 320, 300] }],
        kind: "line",
      },
    },
    "generative-create-task": {
      id: "task",
      kind: "form",
      title: "Create a task",
      description: "Capture a clear next step in this demo.",
      fields: [
        {
          id: "title",
          label: "Title",
          kind: "text",
          value: "Review the shared composer",
          required: true,
        },
        {
          id: "description",
          label: "Description",
          kind: "multiline",
          value: "Check labels, keyboard send, and the narrow layout.",
        },
        { id: "due", label: "Due date", kind: "date", value: "2026-10-03" },
      ],
      submit: { id: "create", label: "Create demo task" },
      cancel: { id: "cancel", label: "Cancel" },
    },
    "generative-software-purchase": {
      id: "purchase",
      kind: "form",
      title: "Choose a demo plan",
      description: "Synthetic pricing · No payment or subscription is created.",
      fields: [
        {
          id: "seats",
          label: "Seats",
          kind: "choice",
          value: "5",
          options: [
            { value: "5", label: "5 seats" },
            { value: "10", label: "10 seats" },
          ],
        },
        {
          id: "frequency",
          label: "Billing frequency",
          kind: "choice",
          value: "monthly",
          options: [
            { value: "monthly", label: "Monthly · $50 demo total" },
            { value: "yearly", label: "Yearly · $500 demo total" },
          ],
        },
        { id: "purpose", label: "Purpose", kind: "multiline", value: "Shared design work" },
        { id: "start", label: "Start date", kind: "date", value: "2026-10-03" },
        { id: "renewal", label: "Automatic renewal", kind: "checkbox", value: "true" },
      ],
      summary: "Demo total: $50 monthly for 5 seats",
      submit: { id: "purchase", label: "Confirm demo selection" },
      cancel: { id: "discard", label: "Discard" },
    },
    "generative-chart-bars": {
      id: "chart",
      kind: "chart",
      chart: {
        label: "Support tickets · Demo",
        labels: ["Mon", "Tue", "Wed", "Thu", "Fri"],
        series: [
          { id: "open", label: "Open", values: [12, 18, 14, 10, 8] },
          { id: "progress", label: "In progress", values: [8, 10, 12, 14, 9] },
          { id: "resolved", label: "Resolved", values: [14, 16, 20, 24, 28] },
        ],
        kind: "bar",
        stacked: true,
      },
    },
  };
  return recipes[slug];
}

export function GenerativeExample({ slug }: { slug: GenerativeSlug }) {
  return <GenerativeRecipeExample key={slug} slug={slug} />;
}

function GenerativeRecipeExample({ slug }: { slug: GenerativeSlug }) {
  const [values, setValues] = useState<Readonly<Record<string, string>>>({});
  const { receipt, record } = useExampleAction();
  const recipe = generativeRecipe(slug);
  if (recipe.kind === "form" && slug === "generative-software-purchase") {
    const seats = Number(values.seats ?? "5");
    const yearly = values.frequency === "yearly";
    recipe.summary = `Demo total: $${seats * (yearly ? 100 : 10)} ${yearly ? "yearly" : "monthly"} for ${seats} seats`;
  }
  return (
    <>
      <GenerativeUI
        node={recipe}
        values={values}
        onFieldChange={(id, value) => {
          setValues((current) => ({ ...current, [id]: value }));
        }}
        onAction={(id, submittedValues) => {
          record(
            `${id}${
              Object.keys(submittedValues ?? values).length > 0
                ? ` · ${Object.entries(submittedValues ?? values)
                    .map(([key, value]) => `${key}: ${value}`)
                    .join(", ")}`
                : ""
            }`,
          );
        }}
      />
      <ExampleReceipt receipt={receipt} />
    </>
  );
}
