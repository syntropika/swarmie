import type { ReactNode } from "react";
import { useState } from "react";

import { Button } from "../../../atoms/button";
import {
  AgentCard,
  AgentHandoff,
  AgentPlan,
  AgentStatus,
  ApprovalCard,
  ArtifactCard,
  BackgroundRuns,
  CheckpointHistory,
  OptionList,
  QuestionFlow,
  RecommendationCard,
  ScheduleCard,
  SubagentList,
  TaskCard,
  TodoList,
} from "../agents";
import type { ApprovalState, TodoItem } from "../agents";
import type { AssistantElementSlugFor } from "../catalog";
import { ComparisonTable } from "../comparison-table";
import { MathBlock } from "../renderers";
import {
  ActivityGraph,
  AssistantChart,
  DataTable,
  Diagram,
  FlowGraph,
  JobProgress,
  MediaPlayer,
  NumberTicker,
  ScoreBreakdown,
  SpecSheet,
  Timeline,
  WebPreview,
} from "../structured";
import {
  demoAudio,
  chartLabels,
  chartSeries,
  ExampleReceipt,
  plan,
  useExampleAction,
} from "./fixtures";

type StructuredAgentSlug = AssistantElementSlugFor<"Structured output" | "Agents">;

const progressFormula = "P = \\frac{\\sum_{i=1}^{n} w_i p_i}{\\sum_{i=1}^{n} w_i}";

export function StructuredAgentExample({ slug }: { slug: StructuredAgentSlug }) {
  const { receipt, record } = useExampleAction();
  const [selected, setSelected] = useState<readonly string[]>([]);
  const [questionIndex, setQuestionIndex] = useState(0);
  const [answers, setAnswers] = useState<Readonly<Record<string, readonly string[]>>>({});
  const [currentCheckpoint, setCurrentCheckpoint] = useState("styles");
  const [committedChoice, setCommittedChoice] = useState<readonly string[]>();
  const [enabled, setEnabled] = useState(true);
  const [approvalState, setApprovalState] = useState<ApprovalState>("request");
  const [todos, setTodos] = useState<TodoItem[]>([
    { id: "read", label: "Read the current contracts", state: "done" },
    { id: "build", label: "Build the shared component", state: "pending" },
    { id: "test", label: "Verify the keyboard flow", state: "pending" },
  ]);
  const facts = [
    { label: "Language", value: "TypeScript" },
    { label: "Owner", value: "Design system", emphasis: true },
    { label: "Rendering", value: "React 19" },
  ];
  const chart = (
    <AssistantChart label="Workspace activity" labels={chartLabels} series={chartSeries} />
  );
  const approval = {
    state: approvalState,
    title: "Apply these component changes?",
    subtitle: "The agent wants to update shared components",
    description: "Update three shared files in this demo workspace.",
    command: "bun run check",
    details: [{ label: "Scope", value: "Three shared files" }],
    allowOnceLabel: "Allow once",
    alwaysAllowLabel: "Always allow",
    denyLabel: "Decline",
    statusLabel: {
      request: "Waiting for approval",
      running: "Approved, running",
      completed: "Finished",
      denied: "Denied",
    }[approvalState],
    onAllowOnce: () => {
      setApprovalState("running");
      record("Changes approved once");
    },
    onAlwaysAllow: () => {
      setApprovalState("running");
      record("Changes always allowed");
    },
    onDeny: () => {
      setApprovalState("denied");
      record("Changes declined");
    },
  };
  const options = [
    { id: "small", label: "Focused component", description: "One clear interaction contract" },
    { id: "composed", label: "Composed surface", description: "A small set of named parts" },
    { id: "catalog", label: "Full catalog", description: "All meaningful states in Storybook" },
  ];
  const renders: Record<StructuredAgentSlug, ReactNode> = {
    "data-table": (
      <DataTable
        caption="Component verification"
        sortAnnouncement={(sort) =>
          sort === null ? "Original order" : `Sorted by ${sort.id}, ${sort.direction}`
        }
        rows={[
          { id: "messages", name: "Messages", count: 12, status: "Ready for review" },
          { id: "tools", name: "Tools", count: 12, status: "Ready for review" },
          { id: "agents", name: "Agents", count: 15, status: "In progress" },
        ]}
        rowId={(row) => row.id}
        emptyLabel="No components"
        columns={[
          {
            id: "name",
            label: "Family",
            render: (row) => row.name,
            compare: (left, right) => left.name.localeCompare(right.name),
          },
          {
            id: "count",
            label: "Components",
            render: (row) => row.count,
            compare: (left, right) => left.count - right.count,
          },
          { id: "status", label: "Status", render: (row) => row.status },
        ]}
      />
    ),
    "number-ticker": (
      <div className="space-y-4">
        <NumberTicker label="Reviewed components" value={selected.length > 0 ? 150 : 128} />
        <Button
          variant="secondary"
          onClick={() => {
            setSelected(["all"]);
          }}
        >
          Include the generative examples
        </Button>
      </div>
    ),
    chart,
    "media-player": <MediaPlayer kind="audio" src={demoAudio} label="Synthesized demo tone" />,
    "web-preview": (
      <WebPreview
        src="https://example.com"
        label="Sandboxed page preview"
        reloadLabel="Reload"
        openLabel="Open page"
      />
    ),
    diagram: (
      <Diagram
        label="Review flow"
        zoomInLabel="Zoom in"
        zoomOutLabel="Zoom out"
        resetLabel="Reset zoom"
        fullscreenLabel="Expand diagram"
        closeLabel="Close diagram"
      >
        <svg
          viewBox="0 0 600 180"
          // oxlint-disable-next-line jsx-a11y/prefer-tag-over-role -- This titled SVG is an authored diagram rather than an external image.
          role="img"
          aria-label="Request, inspect, review"
          className="w-full"
        >
          <title>Request, inspect, review</title>
          <path d="M120 90H480" className="stroke-primary" strokeWidth="2" />
          {[
            { id: "request", x: 80, label: "Request" },
            { id: "inspect", x: 240, label: "Inspect" },
            { id: "review", x: 400, label: "Review" },
          ].map((node) => (
            <g key={node.id}>
              <rect
                x={node.x}
                y="60"
                width="120"
                height="60"
                rx="12"
                className="fill-primary-soft stroke-primary"
              />
              <text x={node.x + 60} y="95" textAnchor="middle" className="fill-ink">
                {node.label}
              </text>
            </g>
          ))}
        </svg>
      </Diagram>
    ),
    "flow-graph": (
      <FlowGraph
        label="Parallel review"
        nodes={[
          {
            id: "request",
            label: "Request",
            x: 20,
            y: 100,
            state: "completed",
            statusLabel: "Done",
          },
          {
            id: "design",
            label: "Design",
            x: 220,
            y: 30,
            state: "running",
            statusLabel: "Running",
          },
          {
            id: "checks",
            label: "Checks",
            x: 220,
            y: 170,
            state: "running",
            statusLabel: "Running",
          },
          { id: "review", label: "Review", x: 450, y: 100, state: "idle", statusLabel: "Next" },
        ]}
        edges={[
          { id: "a", from: "request", to: "design" },
          { id: "b", from: "request", to: "checks" },
          { id: "c", from: "design", to: "review" },
          { id: "d", from: "checks", to: "review" },
        ]}
        onSelect={(id) => {
          record(`Opened ${id}`);
        }}
      />
    ),
    "activity-graph": (
      <ActivityGraph
        label="Session activity"
        days={Array.from({ length: 182 }, (_, index) => ({
          date: new Date(Date.UTC(2026, 3, 1 + index)).toISOString().slice(0, 10),
          value: (index * 7) % 5,
          label: `Demo day ${index + 1}: ${(index * 7) % 5} sessions`,
        }))}
        legend="182 demo days · Lighter cells mean fewer sessions"
      />
    ),
    "math-block": (
      <MathBlock
        label="Weighted progress"
        expression={progressFormula}
        steps={[
          {
            id: "weights",
            expression: "w = [2, 3, 1]",
            explanation: "Give each stage its expected share of the work.",
          },
          {
            id: "progress",
            expression: "p = [1, 0.5, 0]",
            explanation: "Combine completed and partial stages without counting later work.",
          },
        ]}
      />
    ),
    "spec-sheet": (
      <SpecSheet
        title="Shared component contract"
        subtitle="Current shared presentation guarantees"
        items={facts}
      />
    ),
    "comparison-card": (
      <ComparisonTable
        aria-label="Component approaches"
        plans={["Controlled component", "Named parts"]}
        features={[
          { id: "host-state", label: "Host-owned state", values: [true, false] },
          { id: "children", label: "Flexible children", values: [false, true] },
          { id: "context", label: "Shared local context", values: [false, true] },
          { id: "callbacks", label: "Explicit callbacks", values: [true, true] },
        ]}
        featureLabel="Capability"
        includedLabel="Included"
        excludedLabel="Not included"
      />
    ),
    timeline: (
      <Timeline
        label="Run history"
        events={[
          {
            id: "start",
            when: "past",
            title: "Run started",
            detail: "The request entered the workspace",
            time: "12:00",
            state: "completed",
            statusLabel: "Done",
          },
          {
            id: "source",
            when: "past",
            title: "Sources read",
            detail: "Two shared documents",
            time: "12:01",
            state: "completed",
            statusLabel: "Done",
          },
          {
            id: "review",
            when: "now",
            title: "Waiting for review",
            detail: "A person decides the next step",
            time: "12:02",
            state: "waiting",
            statusLabel: "Waiting",
          },
        ]}
      />
    ),
    "job-progress": (
      <JobProgress
        label="Preparing the component catalog"
        eta="About 2 minutes"
        elapsed="38 s"
        {...(receipt === "Job cancelled"
          ? {
              outcome: {
                state: "cancelled" as const,
                label: "Cancelled",
                summary: "The host stopped the pending stages.",
              },
            }
          : {})}
        stages={[
          { id: "inventory", label: "Inventory", weight: 1, progress: 100 },
          { id: "build", label: "Implementation", weight: 3, progress: 70 },
          { id: "verify", label: "Verification", weight: 2, progress: 20 },
        ]}
        cancelLabel="Cancel job"
        onCancel={() => {
          record("Job cancelled");
        }}
      />
    ),
    "score-breakdown": (
      <ScoreBreakdown
        label="Component review criteria"
        total={87}
        outOf={100}
        verdict="Ready for review"
        criteria={[
          {
            id: "access",
            label: "Accessibility",
            score: 9,
            maximum: 10,
            weight: 40,
            explanation: "Focus and labels remain clear.",
          },
          {
            id: "design",
            label: "Design consistency",
            score: 8,
            maximum: 10,
            weight: 35,
            explanation: "All surfaces share the existing tokens.",
          },
          {
            id: "states",
            label: "State coverage",
            score: 7,
            maximum: 10,
            weight: 25,
            explanation: "Additional empty states remain useful.",
          },
        ]}
      />
    ),
    "agent-plan": <AgentPlan title="Implement the shared component" steps={plan} />,
    "subagent-list": (
      <SubagentList
        label="Parallel workers"
        summaryAgent={{
          name: "Summary writer",
          model: "Demo balanced model",
          label: "Summarizing worker results",
        }}
        agents={[
          {
            id: "design",
            name: "Design reviewer",
            task: "Review message surfaces",
            model: "Demo balanced model",
            progress: 75,
            state: "running",
            statusLabel: "Running",
          },
          {
            id: "checks",
            name: "Interaction reviewer",
            task: "Verify keyboard behavior",
            model: "Demo fast model",
            progress: 100,
            state: "completed",
            statusLabel: "Complete",
          },
        ]}
        onSelect={(id) => {
          record(`Opened worker ${id}`);
        }}
      />
    ),
    "task-card": (
      <TaskCard
        title="Review the composer"
        state="completed"
        statusLabel="Complete"
        duration="42 s"
        result={
          <p className="text-ui">
            The composer keeps attachments, model selection, and send actions in one surface.
          </p>
        }
        transcriptLabel="View task transcript"
        transcript={
          <p className="text-ui text-text-muted">
            Read the input contract → verify keyboard send → inspect the narrow layout.
          </p>
        }
      />
    ),
    "agent-status": (
      <AgentStatus state="running" label="Checking the component contracts" elapsed="24 s" />
    ),
    "approval-card": (
      <div className="space-y-3">
        <ApprovalCard {...approval} />
        {approvalState === "running" ? (
          <Button
            variant="secondary"
            onClick={() => {
              setApprovalState("completed");
            }}
          >
            Finish operation
          </Button>
        ) : null}
        {approvalState === "completed" || approvalState === "denied" ? (
          <Button
            variant="ghost"
            onClick={() => {
              setApprovalState("request");
            }}
          >
            Request again
          </Button>
        ) : null}
      </div>
    ),
    "option-list": (
      <OptionList
        label="Which direction should we take?"
        options={options}
        selected={selected}
        onChange={setSelected}
        {...(committedChoice === undefined ? {} : { choice: committedChoice })}
        onConfirm={setCommittedChoice}
      />
    ),
    "question-flow": (
      <QuestionFlow
        questions={[
          { id: "scope", label: "What should we build?", options },
          {
            id: "review",
            label: "How should we review it?",
            options: [
              { id: "storybook", label: "Interactive Storybook" },
              { id: "session", label: "A complete session" },
            ],
          },
        ]}
        index={questionIndex}
        answers={answers}
        onAnswer={(id, answer) => {
          setAnswers((current) => ({ ...current, [id]: answer }));
        }}
        onStepChange={setQuestionIndex}
        onSubmit={() => {
          record(`Answers submitted: ${Object.values(answers).flat().join(", ")}`);
        }}
        backLabel="Back"
        nextLabel="Next"
        submitLabel="Submit answers"
        receipt={receipt.length > 0 ? receipt : undefined}
      />
    ),
    "recommendation-card": (
      <RecommendationCard
        {...approval}
        title="Use controlled presentation contracts"
        description="This keeps execution ownership with the application."
        confidence="High confidence · Based on the frontend guide"
        alternativesLabel="See alternatives"
        onAlternatives={() => {
          record("Consider a host-owned renderer registry as an alternative.");
        }}
      />
    ),
    "artifact-card": (
      <ArtifactCard
        title="Component contract notes"
        detail="Document · 1,240 words"
        version="Version 3 · Updated just now"
        state="completed"
        statusLabel="Ready"
        openLabel="Open document"
        onOpen={() => {
          record("Document opened");
        }}
      />
    ),
    "todo-list": (
      <div className="space-y-3">
        <TodoList label="Working list" items={todos} maxVisible={2} />
        <Button
          variant="secondary"
          onClick={() => {
            setTodos((items) =>
              items.map((item) => (item.id === "build" ? { ...item, state: "active" } : item)),
            );
          }}
        >
          Start next task
        </Button>
      </div>
    ),
    "agent-card": (
      <AgentCard
        name="Swarmie"
        initials="SW"
        description="A participant that helps inspect, coordinate, and explain work."
        skills={["Code review", "Research", "Documentation"]}
        model="Demo balanced model"
        endpoint="Workspace-owned adapter"
        provider="Swarmie workspace"
        version="1.0"
        connected={receipt.length > 0}
        actionLabel={receipt.length > 0 ? "Connected" : "Start a session"}
        onAction={() => {
          record("Session started");
        }}
      />
    ),
    "agent-handoff": (
      <AgentHandoff
        from="Swarmie"
        to="Design reviewer"
        reason="The components are ready for a visual review."
        context={["Current request", "Shared component contracts", "Verification results"]}
        label="Context passed along"
        settled
      />
    ),
    "background-inbox": (
      <BackgroundRuns
        label="Background work"
        runs={[
          {
            id: "build",
            title: "Storybook build",
            detail: "Finished 2 minutes ago",
            state: "completed",
            statusLabel: "Ready",
          },
          {
            id: "checks",
            title: "Interaction checks",
            detail: "12 of 18 scenarios",
            state: "running",
            statusLabel: "Running",
          },
        ]}
        collectLabel="View result"
        onCollect={(id) => {
          record(`Collected ${id}`);
        }}
      />
    ),
    "checkpoint-history": (
      <CheckpointHistory
        label="Restorable checkpoints"
        checkpoints={[
          {
            id: "contracts",
            title: "Contracts added",
            detail: "Restores 8 component files",
            time: "11:42",
          },
          {
            id: "styles",
            title: "Styles adapted",
            detail: "Restores the shared token styling",
            time: "11:55",
          },
        ]}
        currentId={currentCheckpoint}
        restoreLabel="Restore"
        onRestore={(id) => {
          setCurrentCheckpoint(id);
          record(`Restore requested for ${id}`);
        }}
      />
    ),
    "schedule-card": (
      <ScheduleCard
        title="Review workspace activity"
        cadence="Every weekday at 09:00"
        nextRun="Tomorrow · 09:00 Europe/Lisbon"
        enabled={enabled}
        enabledLabel="Schedule enabled"
        onEnabledChange={setEnabled}
        history={<p className="text-xs text-text-muted">Last demo run completed in 18 seconds.</p>}
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
