// @vitest-environment happy-dom
import { createRef, useState } from "react";
import { afterEach, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { Button } from "./components/ui/button";
import { Input } from "./components/ui/input";
import { Textarea } from "./components/ui/textarea";
import { Select } from "./components/ui/select";
import { HarnessSelect } from "./HarnessSelect";
import { Switch } from "./components/ui/switch";
import { Checkbox } from "./components/ui/checkbox";
import { Conversation } from "./Conversation";
import { WorkspaceStore } from "./workspace";
import { Disclosure } from "./components/ui/disclosure";
import { ComboBox } from "./components/ui/combobox";
import { Dialog } from "./components/ui/dialog";
import { RequestCard } from "./RequestCard";
import { ReasoningEffort } from "./ReasoningEffort";

afterEach(() => { cleanup(); vi.restoreAllMocks(); });

it("keeps initially expanded content and reports disclosure changes for lazy loading", async () => {
  const change = vi.fn();
  render(<Disclosure defaultOpen onOpenChange={change}><Disclosure.Summary>Files</Disclosure.Summary><p>Project files</p></Disclosure>);
  const trigger = screen.getByRole("button", { name: "Files" });
  expect(trigger.getAttribute("aria-expanded")).toBe("true");
  trigger.focus();
  await userEvent.keyboard("{Enter}");
  expect(change).toHaveBeenLastCalledWith(false);
  await userEvent.keyboard("{Enter}");
  expect(change).toHaveBeenLastCalledWith(true);
});

it("accepts suggested branches and custom review references", async () => {
  const change = vi.fn();
  function Branches() {
    const [value, setValue] = useState("");
    return <ComboBox label="Review base" value={value} onChange={(next) => { setValue(next); change(next); }} options={["main", "release"]} />;
  }
  render(<Branches />);
  await userEvent.click(screen.getByRole("button", { name: "Show review base suggestions" }));
  await userEvent.click(screen.getByRole("option", { name: "release" }));
  expect(change).toHaveBeenLastCalledWith("release");
  const input = screen.getByRole("combobox", { name: "Review base" });
  await userEvent.clear(input);
  await userEvent.type(input, "HEAD~3");
  await userEvent.keyboard("{ArrowDown}{Enter}");
  expect(change).toHaveBeenLastCalledWith("HEAD~3");
});

it("closes modal dialogs through Escape", async () => {
  const close = vi.fn();
  render(<Dialog label="Find project file" onClose={close}><Input aria-label="File name" /></Dialog>);
  await userEvent.click(screen.getByRole("textbox", { name: "File name" }));
  await userEvent.keyboard("{Escape}");
  expect(close).toHaveBeenCalledOnce();
});

it("submits question choices with their original answer identifiers", async () => {
  const answer = vi.fn().mockResolvedValue(undefined);
  render(<RequestCard permission={{ id: "question", title: "Project preferences", options: [{ id: "submit", label: "Continue" }], metadata: { questions: [
    { id: "base", prompt: "Base branch", options: [{ id: "main", label: "Main" }, { id: "dev", label: "Development" }] },
    { id: "checks", prompt: "Checks", multiple: true, allowText: true, options: [{ id: "lint", label: "Lint" }, { id: "test", label: "Tests" }] },
  ] } }} onAnswer={answer} />);
  await userEvent.click(screen.getByRole("radio", { name: "Development" }));
  await userEvent.click(screen.getByRole("checkbox", { name: "Lint" }));
  await userEvent.click(screen.getByRole("checkbox", { name: "Tests" }));
  await userEvent.click(screen.getByRole("checkbox", { name: "Lint" }));
  await userEvent.type(screen.getByRole("textbox", { name: "Checks written answer" }), "Build");
  await userEvent.click(screen.getByRole("button", { name: "Continue" }));
  await waitFor(() => expect(answer).toHaveBeenLastCalledWith("submit", { answers: { base: "dev", checks: ["test", "Build"] } }));
});

it("keeps form submission and disabled state through native buttons", async () => {
  const submit = vi.fn((event: React.FormEvent) => event.preventDefault());
  const click = vi.fn();
  const view = render(<form onSubmit={submit}><Button type="submit" onClick={click}>Send</Button></form>);
  await userEvent.click(screen.getByRole("button", { name: "Send" }));
  expect(click).toHaveBeenCalledOnce();
  expect(submit).toHaveBeenCalledOnce();
  view.rerender(<form onSubmit={submit}><Button type="submit" disabled onClick={click}>Send</Button></form>);
  await userEvent.click(screen.getByRole("button", { name: "Send" }));
  expect(click).toHaveBeenCalledOnce();
  expect(submit).toHaveBeenCalledOnce();
});

it("preserves DOM refs and pointer handlers used by conversation ordering", () => {
  const ref = createRef<HTMLButtonElement>();
  const pointer = vi.fn();
  render(<Button ref={ref} onPointerDown={pointer} data-thread-id="thread">Conversation</Button>);
  fireEvent.pointerDown(ref.current!, { pointerId: 7 });
  expect(pointer).toHaveBeenCalledOnce();
  expect(ref.current?.dataset.threadId).toBe("thread");
});

it("keeps editable input and textarea values, focus, and native refs", async () => {
  const ref = createRef<HTMLTextAreaElement>();
  function Fields() {
    const [value, setValue] = useState("");
    return <><Input aria-label="Model" value={value} onChange={(event) => setValue(event.target.value)} /><Textarea ref={ref} aria-label="Draft" value={value} onChange={(event) => setValue(event.target.value)} /></>;
  }
  render(<Fields />);
  await userEvent.type(screen.getByRole("textbox", { name: "Model" }), "test-model");
  expect(ref.current?.value).toBe("test-model");
  await userEvent.type(screen.getByRole("textbox", { name: "Draft" }), " update");
  expect((screen.getByRole("textbox", { name: "Model" }) as HTMLInputElement).value).toBe("test-model update");
});

it("returns model identifiers and supports returning to the default choice", async () => {
  const change = vi.fn();
  function Models() {
    const [value, setValue] = useState("");
    return <Select label="Model" value={value} onChange={(next) => { change(next); setValue(next); }} options={[{ id: "", label: "Default model" }, { id: "provider/model", label: "Custom model" }, { id: "unavailable", label: "Unavailable", disabled: true }]} />;
  }
  render(<Models />);
  await userEvent.click(screen.getByRole("combobox", { name: /Model/ }));
  await userEvent.click(screen.getByRole("option", { name: "Custom model" }));
  expect(change).toHaveBeenLastCalledWith("provider/model");
  await userEvent.click(screen.getByRole("combobox", { name: /Model/ }));
  await userEvent.click(screen.getByRole("option", { name: "Default model" }));
  expect(change).toHaveBeenLastCalledWith("");
});

it("reports boolean notification choices through the accessible switch", async () => {
  const change = vi.fn();
  render(<Switch label="Completion notifications" checked={false} onChange={change} />);
  await userEvent.click(screen.getByRole("switch", { name: "Completion notifications" }));
  expect(change).toHaveBeenLastCalledWith(true);
});

it("keeps keyboard selection and disabled state for configuration checkboxes", async () => {
  const change = vi.fn();
  const view = render(<Checkbox label="Isolate worktree" checked={false} onChange={change} />);
  screen.getByRole("checkbox", { name: "Isolate worktree" }).focus();
  await userEvent.keyboard(" ");
  expect(change).toHaveBeenLastCalledWith(true);
  view.rerender(<Checkbox label="Isolate worktree" checked disabled onChange={change} />);
  await userEvent.click(screen.getByRole("checkbox", { name: "Isolate worktree" }));
  expect(change).toHaveBeenCalledOnce();
});

it("closes a keyboard-opened selector with Escape and restores trigger focus", async () => {
  const change = vi.fn();
  render(<Select label="Harness" value="codex" onChange={change} options={[{ id: "codex", label: "codex" }, { id: "unavailable", label: "Not installed", disabled: true }]} />);
  const trigger = screen.getByRole("combobox", { name: /Harness/ });
  trigger.focus();
  await userEvent.keyboard("{Enter}");
  expect(screen.getByRole("listbox", { name: "Harness" })).toBeTruthy();
  expect(screen.getByRole("option", { name: "Not installed" }).getAttribute("aria-disabled")).toBe("true");
  await userEvent.click(screen.getByRole("option", { name: "Not installed" }));
  expect(change).not.toHaveBeenCalled();
  await userEvent.keyboard("{Escape}");
  await waitFor(() => expect(screen.queryByRole("listbox", { name: "Harness" })).toBeNull());
  expect(document.activeElement).toBe(trigger);
});

it("sends exactly once through assistant-ui's slot and retains the stop action", async () => {
  localStorage.clear();
  const store = new WorkspaceStore();
  await store.ready();
  store.createThread();
  const thread = store.active()!;
  const send = vi.spyOn(store, "send").mockResolvedValue();
  const cancel = vi.spyOn(store, "cancel").mockResolvedValue();
  const view = render(<Conversation thread={thread} store={store} controls={null} ready />);
  await userEvent.type(screen.getByRole("textbox", { name: "Message" }), "Inspect this project");
  await userEvent.click(screen.getByRole("button", { name: "Send message" }));
  await waitFor(() => expect(send).toHaveBeenCalledOnce());
  expect(send).toHaveBeenLastCalledWith(thread.id, "Inspect this project", []);
  view.rerender(<Conversation thread={{ ...thread, state: "running" }} store={store} controls={null} ready />);
  await userEvent.click(screen.getByRole("button", { name: "Stop generation" }));
  await waitFor(() => expect(cancel).toHaveBeenCalledOnce());
  expect(cancel).toHaveBeenLastCalledWith(thread.id);
});

it("keeps an unavailable composer disabled without requiring a project", async () => {
  localStorage.clear();
  const store = new WorkspaceStore();
  await store.ready();
  store.createThread();
  const send = vi.spyOn(store, "send").mockResolvedValue();
  render(<Conversation thread={store.active()!} store={store} controls={null} ready={false} />);
  expect((screen.getByRole("textbox", { name: "Message" }) as HTMLTextAreaElement).disabled).toBe(true);
  await userEvent.click(screen.getByRole("button", { name: "Send message" }));
  expect(send).not.toHaveBeenCalled();
  expect(screen.queryByRole("button", { name: "Open project" })).toBeNull();
});

it("shows only installed harnesses with icons and preserves selection", async () => {
  const change = vi.fn();
  render(<HarnessSelect label="Installed harness" value="codex" onChange={change} harnesses={[
    { harness: "codex", available: true },
    { harness: "claude", available: true },
    { harness: "missing", available: false },
  ]} />);
  const trigger = screen.getByRole("combobox", { name: /Installed harness/ });
  expect(trigger.querySelector("svg")).not.toBeNull();
  await userEvent.click(trigger);
  expect(screen.queryByRole("option", { name: "missing" })).toBeNull();
  const claude = screen.getByRole("option", { name: "claude" });
  expect(claude.querySelector("svg")).not.toBeNull();
  await userEvent.click(claude);
  expect(change).toHaveBeenCalledWith("claude");
});

it("disables an empty installed harness list with an honest placeholder", () => {
  render(<HarnessSelect label="Installed harness" value="codex" onChange={vi.fn()} harnesses={[{ harness: "codex", available: false }]} />);
  expect((screen.getByRole("combobox", { name: /Installed harness/ }) as HTMLButtonElement).disabled).toBe(true);
  expect(screen.getByText("No harness installed")).toBeTruthy();
});

it("selects an installed harness from the composer menu and restores focus on dismissal", async () => {
  const change = vi.fn();
  render(<HarnessSelect appearance="menu" label="Harness" value="codex" onChange={change} harnesses={[
    { harness: "codex", available: true },
    { harness: "claude", available: true },
    { harness: "missing", available: false },
  ]} />);
  const trigger = screen.getByRole("button", { name: "Harness: codex" });
  await userEvent.click(trigger);
  expect(screen.queryByRole("menuitemradio", { name: "missing" })).toBeNull();
  expect(screen.getByRole("menuitemradio", { name: "codex" }).getAttribute("aria-checked")).toBe("true");
  await userEvent.click(screen.getByRole("menuitemradio", { name: "codex" }));
  expect(change).not.toHaveBeenCalled();
  await userEvent.click(trigger);
  await userEvent.click(screen.getByRole("menuitemradio", { name: "claude" }));
  expect(change).toHaveBeenCalledOnce();
  expect(change).toHaveBeenCalledWith("claude");
  await waitFor(() => expect(screen.queryByRole("menu")).toBeNull());
  await userEvent.click(trigger);
  await userEvent.keyboard("{Escape}");
  await waitFor(() => expect(screen.queryByRole("menu")).toBeNull());
  expect(document.activeElement).toBe(trigger);
});

it("shows the effective model and configures only advertised models and effort levels", async () => {
  const { SessionControls } = await import("./SessionControls");
  localStorage.clear();
  const store = new WorkspaceStore();
  await store.ready();
  store.createThread();
  const thread = { ...store.active()!, worktreeId: "/Documents", configurations: [
    { id: "model", label: "Model", type: "select" as const, value: "native-a", choices: [{ id: "native-a", label: "Native A" }, { id: "native-b", label: "Native B" }], application: "next_turn" as const },
    { id: "effort", label: "Thinking", type: "select" as const, value: "low", choices: [{ id: "low", label: "Low" }, { id: "high", label: "High" }], application: "next_turn" as const },
  ] };
  const preview = vi.spyOn(store, "previewControls").mockResolvedValue();
  const configure = vi.spyOn(store, "configure").mockResolvedValue();
  render(<SessionControls thread={thread} store={store} connected />);
  await waitFor(() => expect(preview).toHaveBeenCalledWith(thread.id));
  await userEvent.click(screen.getByRole("button", { name: "Select model: Native A, Low" }));
  expect(screen.queryByRole("listbox", { name: "Available models" })).toBeNull();
  const catalogTrigger = screen.getByRole("combobox", { name: "Choose model: Native A" });
  await userEvent.click(catalogTrigger);
  await userEvent.keyboard("{Escape}");
  await waitFor(() => expect(screen.queryByRole("listbox", { name: "Available models" })).toBeNull());
  expect(screen.getByRole("dialog", { name: "Model and session settings" })).toBeTruthy();
  expect(document.activeElement).toBe(catalogTrigger);
  await userEvent.click(catalogTrigger);
  await waitFor(() => expect(screen.getByRole("option", { name: "Native B" }).getAttribute("aria-disabled")).not.toBe("true"));
  expect(screen.queryByText("Default model")).toBeNull();
  await userEvent.click(screen.getByRole("option", { name: "Native B" }));
  expect(configure).toHaveBeenCalledWith(thread.id, "model", "native-b");
  const slider = screen.getByRole("slider", { name: "Reasoning effort" });
  await waitFor(() => expect(slider.getAttribute("aria-disabled")).not.toBe("true"));
  slider.focus();
  await userEvent.keyboard("{ArrowRight}");
  await waitFor(() => expect(configure).toHaveBeenCalledWith(thread.id, "effort", "high"));
});

it("restores the effort selection without giving the lightning a minimum-effort action", async () => {
  const change = vi.fn();
  function EffortPicker() {
    const [value, setValue] = useState("high");
    return <ReasoningEffort choices={[{ id: "low", label: "Low" }, { id: "high", label: "High" }]} value={value} disabled={false}
      modelControl={(label) => <span>{label}</span>}
      onChange={async (next) => { change(next); setValue(next); return true; }} />;
  }
  render(<EffortPicker />);
  expect((screen.getByRole("button", { name: "Reset effort to High" }) as HTMLButtonElement).disabled).toBe(true);
  const slider = screen.getByRole("slider", { name: "Reasoning effort" });
  slider.focus();
  await userEvent.keyboard("{ArrowLeft}");
  expect(change).toHaveBeenLastCalledWith("low");
  await userEvent.click(screen.getByRole("button", { name: "Reset effort to High" }));
  expect(change).toHaveBeenLastCalledWith("high");
  expect(slider.getAttribute("aria-valuetext")).toBe("High");
  expect(screen.queryByRole("button", { name: /Use .* effort/ })).toBeNull();
});

it("uses server permission profiles and keeps selected and observed permissions separate", async () => {
  const { SessionControls } = await import("./SessionControls");
  localStorage.clear();
  const store = new WorkspaceStore();
  await store.ready();
  store.createThread();
  const policy = {
    revision: 1, requested: null, status: "effective" as const, planned: null, plannedProfile: null,
    observed: { profile: null, native: { approvalPolicy: "never", sandbox: "read-only" }, evidence: "native_receipt" as const, coverage: [], limitations: [], nativeVersion: null },
    choices: (["ask", "review", "full"] as const).map((profile) => ({
      profile, label: ({ ask: "Ask for approval", review: "Approve for me", full: "Full access" })[profile],
      description: "Server-provided permission coverage", capability: { availability: "available" as const, source: "harness" as const, evidence: "declared" as const },
      mapping: "equivalent" as const, application: "next_turn" as const,
    })), controlledSettings: [], dispatchBlocked: false, reason: null, bindingId: "binding", nativeSession: null,
  };
  const thread = { ...store.active()!, worktreeId: "/Documents", executionPolicy: policy, configurations: [
    { id: "executionProfile", label: "Execution permissions", type: "select" as const, choices: [], application: "next_turn" as const },
  ] };
  vi.spyOn(store, "previewControls").mockResolvedValue();
  const configure = vi.spyOn(store, "configureMany").mockResolvedValue();
  const view = render(<SessionControls kind="settings" thread={thread} store={store} connected />);
  await userEvent.click(screen.getByRole("button", { name: "Change permissions: Custom permissions" }));
  expect(screen.queryByRole("slider", { name: "Reasoning effort" })).toBeNull();
  await waitFor(() => expect((screen.getByRole("radio", { name: "Approve for me" }) as HTMLButtonElement).disabled).toBe(false));
  await userEvent.click(screen.getByRole("radio", { name: "Approve for me" }));
  expect(configure).toHaveBeenCalledExactlyOnceWith(thread.id, { executionProfile: "review" });
  view.rerender(<SessionControls kind="settings" thread={{ ...thread, executionPolicy: { ...policy, requested: "full", status: "staged" } }} store={store} connected />);
  expect(screen.getByRole("button", { name: "Change permissions: Full access" })).toBeTruthy();
  expect(screen.getByText("Current native permissions: Custom permissions")).toBeTruthy();
  view.rerender(<SessionControls kind="settings" thread={{ ...thread, executionPolicy: {
    ...policy, controlledSettings: ["permissionMode"], choices: policy.choices.map((choice) => choice.profile === "full" ? {
      ...choice, mapping: "partial", capability: { ...choice.capability, availability: "unsupported", reason: "Native deny rules remain unobserved" },
    } : choice),
  }, configurations: [...thread.configurations, {
    id: "permissionMode", label: "Permission mode", type: "select", value: "default", application: "next_turn",
    choices: [{ id: "default", label: "Default" }, { id: "bypassPermissions", label: "Bypass prompts" }],
  }] }} store={store} connected />);
  expect((screen.getByRole("radio", { name: "Full access" }) as HTMLButtonElement).disabled).toBe(true);
  expect(screen.getByText("Native deny rules remain unobserved")).toBeTruthy();
  await userEvent.click(screen.getByText("Native permission settings"));
  fireEvent.change(screen.getByRole("combobox", { name: "Native Permission mode" }), { target: { value: "bypassPermissions" } });
  expect(configure).toHaveBeenLastCalledWith(thread.id, { permissionMode: "bypassPermissions" });
});
