// @vitest-environment happy-dom
import { afterEach, expect, it, vi } from "vitest";
import { cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { HarnessSelect } from "./HarnessSelect";
import { ContextUsage } from "./ContextUsage";
import { sessionPermissions } from "./sessionPermissions";
import { SessionControls } from "./SessionControls";
import { WorkspaceStore } from "./workspace";
import type { Thread } from "./protocol";

afterEach(() => { cleanup(); vi.restoreAllMocks(); });

it("shows status on harness hover or focus and opens its selector on click", async () => {
  const props = { label: "Harness", value: "codex", appearance: "menu" as const, onChange: vi.fn(), harnesses: [{ harness: "codex", available: true }] };
  const view = render(<HarnessSelect {...props} status={{ state: "needs_input", connected: true }} />);
  screen.getByRole("button", { name: "Harness: codex" }).focus();
  await waitFor(() => expect(screen.getByRole("tooltip").textContent).toContain("Waiting for your answer or approval"));
  for (const label of ["Idle", "Running", "Cancelling", "Failed", "Closed / Disconnected"]) expect(screen.getByRole("tooltip").textContent).toContain(label);
  await userEvent.click(screen.getByRole("button", { name: "Harness: codex" }));
  expect(screen.getByRole("menu", { name: "Harness" })).toBeTruthy();
  await userEvent.keyboard("{Escape}");
  view.rerender(<HarnessSelect {...props} status={{ state: "running", connected: false }} />);
  screen.getByRole("button", { name: "Harness: codex" }).focus();
  await waitFor(() => expect(screen.getByRole("tooltip").textContent).toContain("The local workspace is not connected"));
});

it("shows context occupancy from real token counts and handles unknown capacity", async () => {
  const view = render(<ContextUsage context={{ usedTokens: 205000, contextWindowTokens: 258000 }} />);
  screen.getByRole("button", { name: "Context window: 79% full" }).focus();
  await waitFor(() => expect(screen.getByRole("tooltip").textContent).toContain("205k / 258k tokens used"));
  view.rerender(<ContextUsage context={{ usedTokens: 0, contextWindowTokens: 0 }} />);
  expect(screen.getByRole("button", { name: "Context window: usage unavailable" })).toBeTruthy();
  view.rerender(<ContextUsage context={{ usedTokens: 0, contextWindowTokens: 200000 }} />);
  expect(screen.getByRole("button", { name: "Context window: 0% full" })).toBeTruthy();
  view.rerender(<ContextUsage context={undefined} />);
  expect(screen.getByRole("button", { name: "Context window: usage unavailable" })).toBeTruthy();
});

it("does not pretend edit-only permissions provide an automatic approval reviewer", () => {
  const thread = { config: {}, configurations: [{ id: "permissionMode", type: "select", label: "Permissions", live: true, value: "acceptEdits", choices: [{ id: "default", label: "Default" }, { id: "acceptEdits", label: "Accept edits" }, { id: "bypassPermissions", label: "Bypass" }] }] } as Thread;
  const result = sessionPermissions(thread);
  expect(result.label).toBe("Custom permissions");
  expect(result.options.find((option) => option.id === "review")?.available).toBe(false);
  expect(result.options.find((option) => option.id === "ask")?.patch).toEqual({ permissionMode: "default" });
  expect(result.options.find((option) => option.id === "full")?.patch).toEqual({ permissionMode: "bypassPermissions" });
});

it("disables presets whose native settings cannot change in an active session", () => {
  const thread = { sessionId: "native", configurations: [
    { id: "permissionMode", type: "select", label: "Permissions", live: false, value: "default", choices: [{ id: "default", label: "Default" }, { id: "bypassPermissions", label: "Bypass" }] },
  ] } as Thread;
  expect(sessionPermissions(thread).options.find((option) => option.id === "full")).toMatchObject({ available: true, locked: true });
});

it("toggles native Fast mode without changing reasoning effort", async () => {
  localStorage.clear();
  const store = new WorkspaceStore();
  await store.ready(); store.createThread();
  const thread = { ...store.active()!, worktreeId: "/Documents", configurations: [
    { id: "model", type: "select", label: "Model", live: true, value: "native", choices: [{ id: "native", label: "Native" }] },
    { id: "effort", type: "select", label: "Effort", live: true, value: "high", choices: [{ id: "low", label: "Low" }, { id: "high", label: "High" }] },
    { id: "fast", type: "boolean", label: "Fast mode", live: true, value: false },
  ] };
  vi.spyOn(store, "previewControls").mockResolvedValue();
  const configure = vi.spyOn(store, "configure").mockResolvedValue();
  const view = render(<SessionControls thread={thread} store={store} connected />);
  await userEvent.click(screen.getByRole("button", { name: "Select model: Native, High" }));
  await waitFor(() => expect((screen.getByRole("button", { name: "Fast mode" }) as HTMLButtonElement).disabled).toBe(false));
  await userEvent.click(screen.getByRole("button", { name: "Fast mode" }));
  expect(configure).toHaveBeenCalledExactlyOnceWith(thread.id, "fast", true);
  expect(screen.getByRole("slider").getAttribute("aria-valuetext")).toBe("High");
  view.rerender(<SessionControls thread={{ ...thread, configurations: thread.configurations.filter((field) => field.id !== "fast") }} store={store} connected />);
  expect((screen.getByRole("button", { name: "Fast mode" }) as HTMLButtonElement).disabled).toBe(true);
});

it("keeps cached controls while busy and refreshes when idle without a session-start fallback", async () => {
  localStorage.clear();
  const store = new WorkspaceStore();
  await store.ready(); store.createThread();
  const thread = { ...store.active()!, worktreeId: "/Documents", state: "running", configurations: [
    { id: "model", type: "select", label: "Model", live: true, value: "native", choices: [{ id: "native", label: "Native" }] },
  ] };
  const preview = vi.spyOn(store, "previewControls").mockRejectedValue(new Error("session is busy"));
  const view = render(<SessionControls thread={thread} store={store} connected />);
  await userEvent.click(screen.getByRole("button", { name: "Select model: Native" }));
  expect(preview).not.toHaveBeenCalled();
  expect((screen.getByRole("combobox", { name: "Choose model: Native" }) as HTMLButtonElement).disabled).toBe(true);
  view.rerender(<SessionControls thread={{ ...thread, state: "idle" }} store={store} connected />);
  await waitFor(() => expect(preview).toHaveBeenCalledOnce());
  expect(screen.queryByText("session is busy")).toBeNull();
  expect(screen.queryByRole("button", { name: "Start session to load models" })).toBeNull();
});
