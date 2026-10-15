import type { ReactNode } from "react";
import { useState } from "react";

import { Button } from "../../../atoms/button";
import { Icon } from "../../../atoms/icon";
import type { AssistantElementSlugFor } from "../catalog";
import { FileDiff } from "../file-diff";
import type { FileDiffRow } from "../file-diff";
import { InlineCitations } from "../inline-citations";
import {
  DocumentReference,
  GeoMap,
  ImageGallery,
  ImageGeneration,
  LinkPreview,
  MapAnswer,
  MemoryChips,
  ResearchReport,
  Sources,
  RetrievalChunks,
  WebSearch,
} from "../knowledge";
import { projectMapPlaces } from "../map-projection";
import { AssistantPanel, Facts } from "../shared";
import { ThinkingState } from "../thinking-state";
import type { ElicitationField, ReviewableHunk } from "../tools";
import {
  CodeRunner,
  ComputerUse,
  ElicitationForm,
  FileTree,
  PermissionGrant,
  ReviewableDiff,
  ServerPanel,
  TerminalBlock,
  ToolCall,
  ToolFailure,
  ToolTimeline,
} from "../tools";
import { demoImage, diffRows, ExampleReceipt, sources, useExampleAction } from "./fixtures";

function describeDiffRow(row: FileDiffRow) {
  return `${row.kind}: old line ${row.oldLine ?? "none"}, new line ${row.newLine ?? "none"}`;
}

type ToolsKnowledgeSlug = AssistantElementSlugFor<"Tool use" | "Knowledge">;

export function ToolsKnowledgeExample({ slug }: { slug: ToolsKnowledgeSlug }) {
  const { receipt, record } = useExampleAction();
  const [retrying, setRetrying] = useState(false);
  const [permission, setPermission] = useState<"pending" | "session" | "always" | "denied">(
    "pending",
  );
  const [elicitation, setElicitation] = useState<"request" | "accepted" | "declined">("request");
  const [runState, setRunState] = useState<"idle" | "running" | "completed" | "failed">("idle");
  const [imageReady, setImageReady] = useState(false);
  const [activePage, setActivePage] = useState(4);
  const [computerStep, setComputerStep] = useState(0);
  const [toolRunning, setToolRunning] = useState(true);
  const [toolOpen, setToolOpen] = useState(false);
  const [timelineOpen, setTimelineOpen] = useState(false);
  const [fields, setFields] = useState<readonly ElicitationField[]>([
    { id: "remember", label: "Remember this choice", kind: "checkbox", value: "true" },
    { id: "workspace", label: "Workspace", kind: "text", value: "Design team", required: true },
    {
      id: "scope",
      label: "Access scope",
      kind: "choice",
      value: "read",
      options: [
        { value: "read", label: "Read only" },
        { value: "write", label: "Read and write" },
      ],
    },
  ]);
  const [hunks, setHunks] = useState<readonly ReviewableHunk[]>([
    { id: "title", filename: "session-title.ts", rows: diffRows, decision: "pending" },
  ]);
  const [memory, setMemory] = useState([
    { id: "language", text: "Use English for persisted documents", origin: "From this session" },
    { id: "design", text: "Use Swarmie’s shared design tokens", origin: "Workspace guidelines" },
  ]);
  const [selectedPlace, setSelectedPlace] = useState("studio");
  const places = [
    {
      id: "studio",
      name: "Design studio",
      description: "Shared workspace · Demo location",
      latitude: 38.72,
      longitude: -9.14,
    },
    {
      id: "library",
      name: "Reference library",
      description: "Research desk · Demo location",
      latitude: 38.73,
      longitude: -9.13,
    },
  ];
  const projectedPlaces = projectMapPlaces(places);
  const map = (
    <svg
      viewBox="0 0 600 240"
      aria-label="Illustrative map with two demo places"
      className="w-full rounded-control bg-primary-soft"
    >
      <title>Illustrative map with two demo places</title>
      <polyline
        points={projectedPlaces.map((place) => `${place.x},${place.y}`).join(" ")}
        fill="none"
        className="stroke-primary"
        strokeWidth="3"
        strokeDasharray="6 6"
      />
      {projectedPlaces.map((place) => (
        <g key={place.id}>
          <circle
            cx={place.x}
            cy={place.y}
            r={place.id === selectedPlace ? 15 : 10}
            className="fill-primary transition-all duration-200 motion-reduce:transition-none"
          />
          <foreignObject x={place.x - 45} y={place.y + 16} width="90" height="50">
            <Button
              variant="ghost"
              selected={place.id === selectedPlace}
              onClick={() => {
                setSelectedPlace(place.id);
              }}
              className="h-auto w-full text-xs whitespace-normal"
            >
              {place.name}
            </Button>
          </foreignObject>
        </g>
      ))}
    </svg>
  );
  const renders: Record<ToolsKnowledgeSlug, ReactNode> = {
    "tool-call": (
      <div className="space-y-3">
        <ToolCall
          name="Read session contracts"
          activeLabel="Reading session contracts"
          query="frontend.md"
          state={toolRunning ? "running" : "completed"}
          statusLabel={toolRunning ? "Running" : "Completed"}
          open={toolOpen}
          onOpenChange={setToolOpen}
          requestLabel="Request"
          resultLabel="Result"
          request={
            <code className="font-mono text-code">{'{"path":"docs/agents/frontend.md"}'}</code>
          }
          result={
            toolRunning ? undefined : (
              <p>The frontend guide requires controlled shared components.</p>
            )
          }
        />
        <Button
          variant="secondary"
          onClick={() => {
            setToolRunning(!toolRunning);
          }}
        >
          {toolRunning ? "Finish tool" : "Run again"}
        </Button>
      </div>
    ),
    "tool-timeline": (
      <ToolTimeline
        label="Edited 2 files"
        activeLabel="Editing files"
        open={timelineOpen}
        onOpenChange={setTimelineOpen}
        stats={[{ file: "message.tsx", added: 18, removed: 7 }]}
        entries={[
          {
            id: "read",
            action: "Read",
            target: "frontend.md",
            detail: "312 lines",
            state: "completed",
            statusLabel: "Completed",
          },
          {
            id: "write",
            action: "Update",
            target: "message.tsx",
            detail: "+18 −7 lines",
            state: "running",
            statusLabel: "In progress",
          },
        ]}
      />
    ),
    "terminal-block": (
      <TerminalBlock
        command="bun run check"
        cwd="~/workspace"
        duration="1.4 s"
        maxCollapsedLines={2}
        onCopy={(text) => {
          record(`Copied output: ${text}`);
        }}
        lines={[
          { id: "types", text: "\u001B[32m✓ Types checked\u001B[0m" },
          { id: "tests", text: "✓ Interaction tests passed" },
          { id: "warning", text: "Optional preview was skipped", stream: "stderr" },
          { id: "done", text: "All checks completed." },
        ]}
        exitCode={0}
        statusLabel="Exit 0"
      />
    ),
    "code-diff": (
      <FileDiff
        file="session-title.ts"
        rows={diffRows}
        label="Changes to session-title.ts"
        describeRow={describeDiffRow}
      />
    ),
    "reviewable-diff": (
      <ReviewableDiff
        hunks={hunks}
        describeRow={describeDiffRow}
        keepLabel="Keep hunk"
        discardLabel="Discard hunk"
        applyLabel="Apply selected changes"
        summaryLabel={(kept, total) => `${kept} of ${total} hunks kept`}
        onDecision={(id, decision) => {
          setHunks((items) => items.map((item) => (item.id === id ? { ...item, decision } : item)));
        }}
        onApply={(ids) => {
          record(`Applied ${ids.join(", ")}`);
        }}
      />
    ),
    "file-tree": (
      <FileTree
        label="Changed files"
        nodes={[
          {
            id: "ui",
            name: "packages/ui",
            children: [
              {
                id: "message",
                name: "message.tsx",
                additions: 18,
                removals: 7,
                onOpen: () => {
                  record("Opened message.tsx");
                },
              },
              {
                id: "tests",
                name: "message.test.tsx",
                additions: 24,
                removals: 0,
                onOpen: () => {
                  record("Opened message tests");
                },
              },
            ],
          },
        ]}
      />
    ),
    "elicitation-form": (
      <ElicitationForm
        title="Choose the connector’s access"
        state={elicitation}
        statusLabel={elicitation === "accepted" ? "Sent to connector" : "Declined"}
        description="The tool needs a workspace and an access scope before continuing."
        fields={fields}
        onChange={(id, value) => {
          setFields((items) => items.map((item) => (item.id === id ? { ...item, value } : item)));
        }}
        submitLabel="Continue"
        cancelLabel="Decline"
        onSubmit={() => {
          setElicitation("accepted");
          record(`Submitted ${fields.map((field) => field.value).join(", ")}`);
        }}
        onCancel={() => {
          setElicitation("declined");
          record("Elicitation declined");
        }}
      />
    ),
    "mcp-server-panel": (
      <ServerPanel
        label="Connected tools"
        servers={[
          {
            id: "repo",
            name: "Repository",
            state: "completed",
            statusLabel: "Connected",
            tools: ["Read files", "Search code"],
          },
          {
            id: "docs",
            name: "Documentation",
            state: "waiting",
            statusLabel: "Sign-in needed",
            tools: ["Search documents"],
            actionLabel: "Connect",
            onAction: () => {
              record("Documentation connection requested");
            },
          },
        ]}
      />
    ),
    "tool-error": (
      <div className="space-y-3">
        <ToolFailure
          title="Search could not finish"
          message="The documentation source did not respond."
          attempts="Attempt 2 of 3"
          target="Documentation index"
          retrying={retrying}
          retryingLabel="Retrying search"
          skipLabel="Skip tool"
          onSkip={() => {
            record("Skipped failed search");
          }}
          retryLabel="Retry this tool"
          onRetry={() => {
            setRetrying(true);
          }}
        />
        {retrying ? (
          <Button
            variant="ghost"
            onClick={() => {
              setRetrying(false);
            }}
          >
            Simulate failed retry
          </Button>
        ) : null}
      </div>
    ),
    "permission-grant": (
      <PermissionGrant
        title="Allow repository access?"
        state={permission}
        statusLabel={permission === "denied" ? "Access denied" : `Granted for ${permission}`}
        alwaysLabel="Always allow"
        onAlways={() => {
          setPermission("always");
        }}
        scope={[
          { label: "Repository", value: "Demo workspace" },
          { label: "Capability", value: "Read source files" },
          { label: "Duration", value: "This session" },
        ]}
        grantLabel="Allow read access"
        denyLabel="Decline"
        onGrant={() => {
          setPermission("session");
          record("Read access allowed");
        }}
        onDeny={() => {
          setPermission("denied");
          record("Access declined");
        }}
      />
    ),
    "computer-use": (
      <div className="space-y-3">
        <ComputerUse
          label="Inspecting the component preview"
          action="Opening the design documentation"
          steps={[
            { id: "open", x: 20, y: 30 },
            { id: "read", x: 65, y: 60 },
          ]}
          activeIndex={computerStep}
          image={<img src={demoImage} alt="Illustrative document preview" className="w-full" />}
          pauseLabel="Pause"
          onPause={() => {
            record("Computer preview paused");
          }}
        />
        <Button
          variant="ghost"
          onClick={() => {
            setComputerStep((value) => (value + 1) % 2);
          }}
        >
          Next computer step
        </Button>
      </div>
    ),
    "code-runner": (
      <div className="space-y-3">
        <CodeRunner
          language="JavaScript"
          source={'const labels = ["Messages", "Tools", "Agents"];\nconsole.log(labels.length);'}
          runLabel="Run snippet"
          state={runState}
          statusLabel={runState}
          duration={runState === "completed" ? "12 ms" : ""}
          lines={runState === "completed" ? [{ id: "answer", text: "3" }] : []}
          onRun={() => {
            setRunState("running");
          }}
        />
        {runState === "running" ? (
          <Button
            onClick={() => {
              setRunState("completed");
            }}
          >
            Finish snippet
          </Button>
        ) : null}
      </div>
    ),
    "web-search": (
      <WebSearch
        query="Swarmie shared component contracts"
        sources={sources}
        label="Search results"
      />
    ),
    "inline-citation": (
      <InlineCitations
        text="Swarmie uses a warm surface and Dusty Blue actions [1]."
        references={sources.map((source, index) => ({
          number: index + 1,
          label: source.title,
          host: source.domain ?? new URL(source.href).host,
          url: source.href,
        }))}
        sourcesLabel="Sources"
      />
    ),
    "link-preview": <LinkPreview source={sources[0]} />,
    "image-generation": (
      <div className="space-y-3">
        <ImageGeneration
          label="Generating a document illustration"
          progress={imageReady ? 100 : 62}
          onRegenerate={() => {
            setImageReady(false);
          }}
          regenerateLabel="Regenerate image"
        >
          {imageReady ? (
            <img
              src={demoImage}
              alt="Generated illustration fixture"
              className="w-full rounded-control"
            />
          ) : null}
        </ImageGeneration>
        <Button
          variant="secondary"
          onClick={() => {
            setImageReady(true);
            record("Image resolved");
          }}
        >
          Resolve example image
        </Button>
      </div>
    ),
    "image-gallery": (
      <ImageGallery
        label="Image results"
        images={[
          { id: "a", src: demoImage, alt: "Document illustration", caption: "Shared document" },
          {
            id: "b",
            src: demoImage,
            alt: "Second document illustration",
            caption: "Working notes",
          },
        ]}
        onSelect={(image) => {
          record(`Previewing ${image.alt}`);
        }}
      />
    ),
    "retrieval-chunks": (
      <RetrievalChunks
        label="Retrieved passages"
        chunks={[
          {
            id: "shared",
            title: "Shared UI first",
            text: "Reuse an existing primitive, extend its contract, or add a neutral primitive with a clear shared responsibility.",
            relevance: "94% relevant",
            score: 0.94,
            href: sources[1].href,
          },
          {
            id: "control",
            title: "Controlled views",
            text: "Views receive data and callbacks; controllers own application behavior.",
            relevance: "88% relevant",
            score: 0.88,
            href: sources[1].href,
          },
        ]}
      />
    ),
    "document-reference": (
      <DocumentReference
        source={sources[0]}
        page="Working surfaces · Section 4"
        anchors={[
          { page: 4, label: "Page 4", quote: "Use meaningful grouping." },
          { page: 6, label: "Page 6", quote: "Keep controls accessible." },
        ]}
        activePage={activePage}
        onJump={setActivePage}
        quote="Make coordination visible through aligned edges, meaningful grouping, and clear hierarchy."
      />
    ),
    "memory-chips": (
      <MemoryChips
        label="Remembered preferences"
        entries={memory}
        removeLabel="Remove"
        onRemove={(id) => {
          setMemory((items) => items.filter((item) => item.id !== id));
          record("Memory removed");
        }}
      />
    ),
    "research-report": (
      <ResearchReport
        title="Shared component research"
        sourcesLabel="Section sources"
        sections={[
          {
            id: "contracts",
            title: "Contract ownership",
            content: (
              <p className="text-ui">
                Swarmie components receive data and callbacks without requiring a provider runtime.
              </p>
            ),
            state: "done",
            sources,
          },
          {
            id: "states",
            title: "Interaction states",
            content: <ThinkingState>Comparing error and cancellation states</ThinkingState>,
            state: "writing",
            sources: [],
          },
        ]}
      />
    ),
    "map-answer": (
      <MapAnswer
        label="Workspace locations"
        places={places}
        map={map}
        selectedId={selectedPlace}
        onSelect={setSelectedPlace}
      />
    ),
    "geo-map": (
      <GeoMap
        label="Workspace geography"
        places={places}
        map={map}
        selectedId={selectedPlace}
        onSelect={setSelectedPlace}
        attribution="Illustrative geographic fixture. Hosts supply their tile provider through the map slot."
      />
    ),
  };
  const content = renders[slug];
  return (
    <>
      {content}
      <ExampleReceipt receipt={receipt} />
    </>
  );
}

export function SourcesExample() {
  const [open, setOpen] = useState(false);
  return (
    <Sources
      label="Sources"
      sources={sources.map((source, index) => ({
        ...source,
        ...(index === 0 ? { author: "Swarmie", publishedAt: "2026-09-23" } : {}),
      }))}
      open={open}
      onOpenChange={setOpen}
    />
  );
}

export type ToolUIPhase = "arguments" | "running" | "completed" | "failed";

type WeatherInvocation =
  | { phase: "arguments"; args: { location: string } }
  | { phase: "running"; args: { location: string } }
  | {
      phase: "completed";
      args: { location: string };
      result: { temperature: number; condition: string; humidity: number; windSpeed: number };
    }
  | { phase: "failed"; args: { location: string }; message: string };

function unexpectedWeatherState(value: never): never {
  throw new Error(`Unexpected weather state: ${String(value)}`);
}

function weatherInvocation(phase: ToolUIPhase): WeatherInvocation {
  switch (phase) {
    case "arguments": {
      return { phase, args: { location: "San Fra…" } };
    }
    case "running": {
      return { phase, args: { location: "San Francisco" } };
    }
    case "completed": {
      return {
        phase,
        args: { location: "San Francisco" },
        result: { temperature: 72, condition: "Sunny", humidity: 45, windSpeed: 8 },
      };
    }
    case "failed": {
      return {
        phase,
        args: { location: "San Francisco" },
        message: "The weather source did not respond.",
      };
    }
    default: {
      return unexpectedWeatherState(phase);
    }
  }
}

/** A synthetic host adapter supplies args, lifecycle, and rich output to the same tool renderer. */
export function ToolUIExample({ initialPhase = "arguments" }: { initialPhase?: ToolUIPhase }) {
  const [phase, setPhase] = useState(initialPhase);
  const invocation = weatherInvocation(phase);
  const statusLabels = {
    arguments: "Receiving tool arguments",
    running: "Checking the forecast",
    completed: "Forecast ready",
    failed: "Weather unavailable",
  };
  let output: ReactNode;
  switch (invocation.phase) {
    case "arguments": {
      output = <ThinkingState>Receiving location: {invocation.args.location}</ThinkingState>;
      break;
    }
    case "running": {
      output = (
        <div className="flex items-center gap-2">
          <Icon name="loading" className="size-4 motion-safe:animate-assistant-spin" />
          <ThinkingState>Checking weather in {invocation.args.location}…</ThinkingState>
        </div>
      );
      break;
    }
    case "completed": {
      output = (
        <AssistantPanel
          aria-label={`Weather in ${invocation.args.location}`}
          className="bg-surface-muted"
        >
          <div className="mb-3 flex items-center justify-between gap-3">
            <div>
              <h3 className="font-medium">{invocation.args.location}</h3>
              <p className="text-xs text-text-muted">Current weather</p>
            </div>
            <Icon name="world" className="text-primary" />
          </div>
          <div className="mb-3 flex items-baseline gap-3">
            <span className="text-document">{invocation.result.temperature}°F</span>
            <span className="text-text-muted">{invocation.result.condition}</span>
          </div>
          <Facts
            items={[
              { label: "Humidity", value: `${invocation.result.humidity}%` },
              { label: "Wind", value: `${invocation.result.windSpeed} mph` },
            ]}
          />
        </AssistantPanel>
      );
      break;
    }
    case "failed": {
      output = (
        <ToolFailure
          title="Weather unavailable"
          message={invocation.message}
          attempts="Attempt 1 of 3"
          retryLabel="Retry forecast"
          onRetry={() => {
            setPhase("running");
          }}
        />
      );
      break;
    }
    default: {
      output = unexpectedWeatherState(invocation);
    }
  }
  return (
    <div className="space-y-4">
      <div className="text-content leading-content">
        <p>What’s the weather in San Francisco?</p>
      </div>
      <section
        aria-label="Weather tool invocation"
        aria-busy={phase === "arguments" || phase === "running"}
        className="space-y-2"
      >
        <p className="text-xs text-text-muted">{statusLabels[phase]}</p>
        {output}
      </section>
      {phase === "completed" ? (
        <p className="text-content leading-content">It’s sunny and 72°F in San Francisco.</p>
      ) : null}
      <div className="flex flex-wrap gap-2" aria-label="Demo lifecycle controls">
        <Button
          variant="secondary"
          onClick={() => {
            setPhase("running");
          }}
        >
          Start tool
        </Button>
        <Button
          variant="secondary"
          onClick={() => {
            setPhase("completed");
          }}
        >
          Resolve forecast
        </Button>
        <Button
          variant="ghost"
          onClick={() => {
            setPhase("failed");
          }}
        >
          Fail tool
        </Button>
        <Button
          variant="ghost"
          onClick={() => {
            setPhase("arguments");
          }}
        >
          Replay arguments
        </Button>
      </div>
    </div>
  );
}
