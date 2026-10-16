import {
  Button,
  Checkbox,
  Disclosure,
  Input,
  Switch,
  TextArea,
  ComboBox,
  ListBox,
  Modal,
  Select,
} from "@swarmie/ui-web";
import { cleanup, fireEvent, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { isValidElement, createRef, useState } from "react";
// @vitest-environment happy-dom
import { afterEach, expect, it, vi } from "vitest";

import { useConfirmation } from "~/src/components/confirmation";
import { Conversation } from "~/src/conversation";
import { HarnessSelect } from "~/src/harness-select";
import { ReasoningEffort } from "~/src/reasoning-effort";
import { RequestCard } from "~/src/request-card";
import { renderWithQuery as render } from "~/src/test-fixtures";
import { WorkspaceStore } from "~/src/workspace";

function renderEffortLabel(label: string) {
  return <span>{label}</span>;
}
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});
it("keeps initially expanded content and reports disclosure changes for lazy loading", async () => {
  const change = vi.fn<(...args: unknown[]) => void>();
  render(
    <Disclosure defaultExpanded onExpandedChange={change}>
      <Disclosure.Heading>
        <Button slot="trigger" variant="tertiary">
          <span>Files</span>
          <Disclosure.Indicator />
        </Button>
      </Disclosure.Heading>
      <Disclosure.Content>
        <Disclosure.Body>
          <p>Project files</p>
        </Disclosure.Body>
      </Disclosure.Content>
    </Disclosure>,
  );
  const trigger = screen.getByRole("button", {
    name: "Files",
  });
  expect(trigger.getAttribute("aria-expanded")).toBe("true");
  trigger.focus();
  await userEvent.keyboard("{Enter}");
  expect(change).toHaveBeenLastCalledWith(false);
  await userEvent.keyboard("{Enter}");
  expect(change).toHaveBeenLastCalledWith(true);
});
it("accepts suggested branches and custom review references", async () => {
  const change = vi.fn<(...args: unknown[]) => void>();
  function Branches() {
    const [value, setValue] = useState("");
    return (
      <ComboBox
        aria-label="Review base"
        allowsCustomValue
        inputValue={value}
        onInputChange={(next) => {
          setValue(next);
          change(next);
        }}
        menuTrigger="manual"
      >
        <ComboBox.InputGroup>
          <Input />
          <ComboBox.Trigger aria-label="Show review base suggestions" />
        </ComboBox.InputGroup>
        <ComboBox.Popover>
          <ListBox
            aria-label="Review base suggestions"
            renderEmptyState={() => "No matching suggestions. You can use the typed reference."}
          >
            {["main", "release"]
              .filter((option) => option.toLocaleLowerCase().includes(value.toLocaleLowerCase()))
              .map((option) => (
                <ListBox.Item key={option} id={option} textValue={option}>
                  {option}
                  <ListBox.ItemIndicator />
                </ListBox.Item>
              ))}
          </ListBox>
        </ComboBox.Popover>
      </ComboBox>
    );
  }
  render(<Branches />);
  await userEvent.click(
    screen.getByRole("button", {
      name: "Show review base suggestions",
    }),
  );
  await userEvent.click(
    screen.getByRole("option", {
      name: "release",
    }),
  );
  expect(change).toHaveBeenLastCalledWith("release");
  const input = screen.getByRole("combobox", {
    name: "Review base",
  });
  await userEvent.clear(input);
  await userEvent.type(input, "HEAD~3");
  await userEvent.keyboard("{ArrowDown}{Enter}");
  expect(change).toHaveBeenLastCalledWith("HEAD~3");
});
it("closes modal dialogs through Escape", async () => {
  const close = vi.fn<(...args: unknown[]) => void>();
  render(
    <Modal
      isOpen
      onOpenChange={(next) => {
        if (!next) {
          close();
        }
      }}
    >
      <Modal.Backdrop isDismissable>
        <Modal.Container>
          <Modal.Dialog aria-label="Find project file">
            <Input aria-label="File name" />
          </Modal.Dialog>
        </Modal.Container>
      </Modal.Backdrop>
    </Modal>,
  );
  await userEvent.click(
    screen.getByRole("textbox", {
      name: "File name",
    }),
  );
  await userEvent.keyboard("{Escape}");
  expect(close).toHaveBeenCalledOnce();
});
it("submits question choices with their original answer identifiers", async () => {
  const answer = vi.fn().mockResolvedValue(undefined);
  render(
    <RequestCard
      permission={{
        id: "question",
        title: "Project preferences",
        options: [
          {
            id: "submit",
            label: "Continue",
          },
        ],
        metadata: {
          questions: [
            {
              id: "base",
              prompt: "Base branch",
              options: [
                {
                  id: "main",
                  label: "Main",
                },
                {
                  id: "dev",
                  label: "Development",
                },
              ],
            },
            {
              id: "checks",
              prompt: "Checks",
              multiple: true,
              allowText: true,
              options: [
                {
                  id: "lint",
                  label: "Lint",
                },
                {
                  id: "test",
                  label: "Tests",
                },
              ],
            },
          ],
        },
      }}
      onAnswer={answer}
    />,
  );
  await userEvent.click(
    screen.getByRole("radio", {
      name: "Development",
    }),
  );
  await userEvent.click(
    screen.getByRole("checkbox", {
      name: "Lint",
    }),
  );
  await userEvent.click(
    screen.getByRole("checkbox", {
      name: "Tests",
    }),
  );
  await userEvent.click(
    screen.getByRole("checkbox", {
      name: "Lint",
    }),
  );
  await userEvent.type(
    screen.getByRole("textbox", {
      name: "Checks written answer",
    }),
    "Build",
  );
  await userEvent.click(
    screen.getByRole("button", {
      name: "Continue",
    }),
  );
  await waitFor(() => {
    expect(answer).toHaveBeenLastCalledWith("submit", {
      answers: {
        base: "dev",
        checks: ["test", "Build"],
      },
    });
  });
});
it("keeps form submission and disabled state through native buttons", async () => {
  const submit = vi.fn((event: React.SyntheticEvent<HTMLFormElement>) => {
    event.preventDefault();
  });
  const click = vi.fn<(...args: unknown[]) => void>();
  const view = render(
    <form onSubmit={submit}>
      <Button type="submit" onClick={click}>
        Send
      </Button>
    </form>,
  );
  await userEvent.click(
    screen.getByRole("button", {
      name: "Send",
    }),
  );
  expect(click).toHaveBeenCalledOnce();
  expect(submit).toHaveBeenCalledOnce();
  view.rerender(
    <form onSubmit={submit}>
      <Button type="submit" disabled onClick={click}>
        Send
      </Button>
    </form>,
  );
  await userEvent.click(
    screen.getByRole("button", {
      name: "Send",
    }),
  );
  expect(click).toHaveBeenCalledOnce();
  expect(submit).toHaveBeenCalledOnce();
});
it("preserves DOM refs and pointer handlers used by conversation ordering", () => {
  const ref = createRef<HTMLButtonElement>();
  const pointer = vi.fn<(...args: unknown[]) => void>();
  render(
    <Button ref={ref} onPointerDown={pointer} data-thread-id="thread">
      Conversation
    </Button>,
  );
  fireEvent.pointerDown(requireFixtureValue(ref.current), {
    pointerId: 7,
  });
  expect(pointer).toHaveBeenCalledOnce();
  expect(ref.current?.dataset.threadId).toBe("thread");
});
it("keeps editable input and textarea values, focus, and native refs", async () => {
  const ref = createRef<HTMLTextAreaElement>();
  function Fields() {
    const [value, setValue] = useState("");
    return (
      <>
        <Input
          aria-label="Model"
          value={value}
          onChange={(event) => {
            setValue(event.target.value);
          }}
        />
        <TextArea
          ref={ref}
          aria-label="Draft"
          value={value}
          onChange={(event) => {
            setValue(event.target.value);
          }}
        />
      </>
    );
  }
  render(<Fields />);
  await userEvent.type(
    screen.getByRole("textbox", {
      name: "Model",
    }),
    "test-model",
  );
  expect(ref.current?.value).toBe("test-model");
  await userEvent.type(
    screen.getByRole("textbox", {
      name: "Draft",
    }),
    " update",
  );
  expect(
    screen.getByRole<HTMLInputElement>("textbox", {
      name: "Model",
    }).value,
  ).toBe("test-model update");
});
it("returns model identifiers and supports returning to the default choice", async () => {
  const change = vi.fn<(...args: unknown[]) => void>();
  function Models() {
    const [value, setValue] = useState("");
    return (
      <Select
        aria-label="Model"
        value={value}
        onChange={(key) => {
          if (typeof key === "string") {
            ((next) => {
              change(next);
              setValue(next);
            })(key);
          }
        }}
        placeholder="Select…"
      >
        <Select.Trigger>
          <Select.Value />
          <Select.Indicator />
        </Select.Trigger>
        <Select.Popover>
          <ListBox aria-label="Model">
            {[
              {
                id: "",
                label: "Default model",
              },
              {
                id: "provider/model",
                label: "Custom model",
              },
              {
                id: "unavailable",
                label: "Unavailable",
                disabled: true,
              },
            ].map((option) => (
              <ListBox.Item
                key={option.id}
                id={option.id}
                textValue={option.label}
                isDisabled={"disabled" in option && option.disabled === true}
              >
                {"icon" in option && isValidElement(option.icon) ? option.icon : null}
                {option.label}
                <ListBox.ItemIndicator />
              </ListBox.Item>
            ))}
          </ListBox>
        </Select.Popover>
      </Select>
    );
  }
  render(<Models />);
  await userEvent.click(
    screen.getByRole("button", {
      name: /Model/u,
    }),
  );
  await userEvent.click(
    screen.getByRole("option", {
      name: "Custom model",
    }),
  );
  expect(change).toHaveBeenLastCalledWith("provider/model");
  await userEvent.click(
    screen.getByRole("button", {
      name: /Model/u,
    }),
  );
  await userEvent.click(
    screen.getByRole("option", {
      name: "Default model",
    }),
  );
  expect(change).toHaveBeenLastCalledWith("");
});
it("reports boolean notification choices through the accessible switch", async () => {
  const change = vi.fn<(...args: unknown[]) => void>();
  render(
    <Switch isSelected={false} onChange={change} aria-label="Completion notifications">
      <Switch.Content>
        <Switch.Control>
          <Switch.Thumb />
        </Switch.Control>
        Completion notifications
      </Switch.Content>
    </Switch>,
  );
  await userEvent.click(
    screen.getByRole("switch", {
      name: "Completion notifications",
    }),
  );
  expect(change).toHaveBeenLastCalledWith(true);
});
it("keeps keyboard selection and disabled state for configuration checkboxes", async () => {
  const change = vi.fn<(...args: unknown[]) => void>();
  const view = render(
    <Checkbox isSelected={false} onChange={change} aria-label="Isolate worktree">
      <Checkbox.Content>
        <Checkbox.Control>
          <Checkbox.Indicator />
        </Checkbox.Control>
        Isolate worktree
      </Checkbox.Content>
    </Checkbox>,
  );
  screen
    .getByRole("checkbox", {
      name: "Isolate worktree",
    })
    .focus();
  await userEvent.keyboard(" ");
  expect(change).toHaveBeenLastCalledWith(true);
  view.rerender(
    <Checkbox isSelected isDisabled onChange={change} aria-label="Isolate worktree">
      <Checkbox.Content>
        <Checkbox.Control>
          <Checkbox.Indicator />
        </Checkbox.Control>
        Isolate worktree
      </Checkbox.Content>
    </Checkbox>,
  );
  await userEvent.click(
    screen.getByRole("checkbox", {
      name: "Isolate worktree",
    }),
  );
  expect(change).toHaveBeenCalledOnce();
});
it("closes a keyboard-opened selector with Escape and restores trigger focus", async () => {
  const change = vi.fn<(...args: unknown[]) => void>();
  render(
    <Select
      aria-label="Harness"
      value="codex"
      onChange={(key) => {
        if (typeof key === "string") {
          change(key);
        }
      }}
      placeholder="Select…"
    >
      <Select.Trigger>
        <Select.Value />
        <Select.Indicator />
      </Select.Trigger>
      <Select.Popover>
        <ListBox aria-label="Harness">
          {[
            {
              id: "codex",
              label: "codex",
            },
            {
              id: "unavailable",
              label: "Not installed",
              disabled: true,
            },
          ].map((option) => (
            <ListBox.Item
              key={option.id}
              id={option.id}
              textValue={option.label}
              isDisabled={"disabled" in option && option.disabled === true}
            >
              {"icon" in option && isValidElement(option.icon) ? option.icon : null}
              {option.label}
              <ListBox.ItemIndicator />
            </ListBox.Item>
          ))}
        </ListBox>
      </Select.Popover>
    </Select>,
  );
  const trigger = screen.getByRole("button", {
    name: /Harness/u,
  });
  trigger.focus();
  await userEvent.keyboard("{Enter}");
  expect(
    await screen.findByRole("listbox", {
      name: /Harness/u,
    }),
  ).toBeTruthy();
  expect(
    screen
      .getByRole("option", {
        name: "Not installed",
      })
      .getAttribute("aria-disabled"),
  ).toBe("true");
  await userEvent.click(
    screen.getByRole("option", {
      name: "Not installed",
    }),
  );
  expect(change).not.toHaveBeenCalled();
  await userEvent.keyboard("{Escape}");
  await waitFor(() => {
    expect(
      screen.queryByRole("listbox", {
        name: "Harness",
      }),
    ).toBeNull();
  });
  expect(document.activeElement).toBe(trigger);
});
it("sends exactly once through assistant-ui's slot and retains the stop action", async () => {
  localStorage.clear();
  const store = new WorkspaceStore();
  await store.ready();
  store.createThread();
  const thread = requireFixtureValue(store.active());
  const send = vi.spyOn(store, "send").mockResolvedValue();
  const cancel = vi.spyOn(store, "cancel").mockResolvedValue();
  const view = render(<Conversation thread={thread} store={store} controls={null} ready />);
  await userEvent.type(
    screen.getByRole("textbox", {
      name: "Message",
    }),
    "Inspect this project",
  );
  await userEvent.click(
    screen.getByRole("button", {
      name: "Send message",
    }),
  );
  await waitFor(() => {
    expect(send).toHaveBeenCalledOnce();
  });
  expect(send).toHaveBeenLastCalledWith(thread.id, "Inspect this project", []);
  view.rerender(
    <Conversation
      thread={{
        ...thread,
        state: "running",
      }}
      store={store}
      controls={null}
      ready
    />,
  );
  await userEvent.click(
    screen.getByRole("button", {
      name: "Stop generation",
    }),
  );
  await waitFor(() => {
    expect(cancel).toHaveBeenCalledOnce();
  });
  expect(cancel).toHaveBeenLastCalledWith(thread.id);
});
it("keeps an unavailable composer disabled without requiring a project", async () => {
  localStorage.clear();
  const store = new WorkspaceStore();
  await store.ready();
  store.createThread();
  const send = vi.spyOn(store, "send").mockResolvedValue();
  render(
    <Conversation
      thread={requireFixtureValue(store.active())}
      store={store}
      controls={null}
      ready={false}
    />,
  );
  expect(
    screen.getByRole<HTMLTextAreaElement>("textbox", {
      name: "Message",
    }).disabled,
  ).toBe(true);
  await userEvent.click(
    screen.getByRole("button", {
      name: "Send message",
    }),
  );
  expect(send).not.toHaveBeenCalled();
  expect(
    screen.queryByRole("button", {
      name: "Open project",
    }),
  ).toBeNull();
});
it("shows only installed agent tools with icons and preserves selection", async () => {
  const change = vi.fn<(...args: unknown[]) => void>();
  render(
    <HarnessSelect
      label="Installed agent tool"
      value="codex"
      onChange={change}
      harnesses={[
        {
          harness: "codex",
          available: true,
        },
        {
          harness: "claude",
          available: true,
        },
        {
          harness: "missing",
          available: false,
        },
      ]}
    />,
  );
  const trigger = screen.getByRole("button", {
    name: /Installed agent tool/u,
  });
  expect(trigger.querySelector("svg")).not.toBeNull();
  await userEvent.click(trigger);
  expect(
    screen.queryByRole("option", {
      name: "missing",
    }),
  ).toBeNull();
  const claude = screen.getByRole("option", {
    name: "Claude Code",
  });
  expect(claude.querySelector("svg")).not.toBeNull();
  await userEvent.click(claude);
  expect(change).toHaveBeenCalledWith("claude");
});
it("disables an empty installed agent list with an honest placeholder", () => {
  render(
    <HarnessSelect
      label="Installed agent tool"
      value="codex"
      onChange={vi.fn<(...args: unknown[]) => void>()}
      harnesses={[
        {
          harness: "codex",
          available: false,
        },
      ]}
    />,
  );
  expect(
    screen.getByRole<HTMLButtonElement>("button", {
      name: /Installed agent tool/u,
    }).disabled,
  ).toBe(true);
  expect(screen.getByText("No agents installed")).toBeTruthy();
});
it("selects an installed agent from the composer menu and restores focus on dismissal", async () => {
  const change = vi.fn<(...args: unknown[]) => void>();
  render(
    <HarnessSelect
      appearance="menu"
      label="Agent tool"
      value="codex"
      onChange={change}
      harnesses={[
        {
          harness: "codex",
          available: true,
        },
        {
          harness: "claude",
          available: true,
        },
        {
          harness: "missing",
          available: false,
        },
      ]}
    />,
  );
  const trigger = screen.getByRole("button", {
    name: "Agent tool: Codex",
  });
  await userEvent.click(trigger);
  expect(
    screen.queryByRole("menuitemradio", {
      name: "missing",
    }),
  ).toBeNull();
  expect(
    screen
      .getByRole("menuitemradio", {
        name: "Codex",
      })
      .getAttribute("aria-checked"),
  ).toBe("true");
  await userEvent.click(
    screen.getByRole("menuitemradio", {
      name: "Codex",
    }),
  );
  expect(change).not.toHaveBeenCalled();
  await userEvent.click(trigger);
  await userEvent.click(
    screen.getByRole("menuitemradio", {
      name: "Claude Code",
    }),
  );
  expect(change).toHaveBeenCalledOnce();
  expect(change).toHaveBeenCalledWith("claude");
  await waitFor(() => {
    expect(screen.queryByRole("menu")).toBeNull();
  });
  await userEvent.click(trigger);
  await userEvent.keyboard("{Escape}");
  await waitFor(() => {
    expect(screen.queryByRole("menu")).toBeNull();
  });
  expect(document.activeElement).toBe(trigger);
});
it("shows the effective model and configures only advertised models and effort levels", async () => {
  const { SessionControls } = await import("~/src/session-controls");
  localStorage.clear();
  const store = new WorkspaceStore();
  await store.ready();
  store.createThread();
  const thread = {
    ...requireFixtureValue(store.active()),
    worktreeId: "/Documents",
    configurations: [
      {
        id: "model",
        label: "Model",
        type: "select" as const,
        value: "native-a",
        choices: [
          {
            id: "native-a",
            label: "Native A",
          },
          {
            id: "native-b",
            label: "Native B",
          },
        ],
        application: "next_turn" as const,
      },
      {
        id: "effort",
        label: "Thinking",
        type: "select" as const,
        value: "low",
        choices: [
          {
            id: "low",
            label: "Low",
          },
          {
            id: "high",
            label: "High",
          },
        ],
        application: "next_turn" as const,
      },
    ],
  };
  const preview = vi.spyOn(store, "previewControls").mockResolvedValue();
  const configure = vi.spyOn(store, "configure").mockResolvedValue();
  render(<SessionControls thread={thread} store={store} connected />);
  await waitFor(() => {
    expect(preview).toHaveBeenCalledWith(thread.id);
  });
  await userEvent.click(
    screen.getByRole("button", {
      name: "Select model: Native A, Low",
    }),
  );
  expect(
    screen.queryByRole("listbox", {
      name: "Available models",
    }),
  ).toBeNull();
  const catalogTrigger = screen.getByRole("button", {
    name: /Choose model: Native A/u,
  });
  await userEvent.click(catalogTrigger);
  await userEvent.keyboard("{Escape}");
  await waitFor(() => {
    expect(
      screen.queryByRole("listbox", {
        name: "Available models",
      }),
    ).toBeNull();
  });
  expect(
    screen.getByRole("dialog", {
      name: "Model and session settings",
    }),
  ).toBeTruthy();
  expect(document.activeElement).toBe(catalogTrigger);
  await userEvent.click(catalogTrigger);
  await waitFor(() => {
    expect(
      screen
        .getByRole("option", {
          name: "Native B",
        })
        .getAttribute("aria-disabled"),
    ).not.toBe("true");
  });
  expect(screen.queryByText("Default model")).toBeNull();
  await userEvent.click(
    screen.getByRole("option", {
      name: "Native B",
    }),
  );
  expect(configure).toHaveBeenCalledWith(thread.id, "model", "native-b");
  const slider = screen.getByRole("slider", {
    name: "Reasoning effort",
  });
  await waitFor(() => {
    expect(slider.getAttribute("aria-disabled")).not.toBe("true");
  });
  slider.focus();
  await userEvent.keyboard("{ArrowRight}");
  await waitFor(() => {
    expect(configure).toHaveBeenCalledWith(thread.id, "effort", "high");
  });
});
it("restores the effort selection without giving the lightning a minimum-effort action", async () => {
  const change = vi.fn<(...args: unknown[]) => void>();
  function EffortPicker() {
    const [value, setValue] = useState("high");
    return (
      <ReasoningEffort
        choices={[
          {
            id: "low",
            label: "Low",
          },
          {
            id: "high",
            label: "High",
          },
        ]}
        value={value}
        disabled={false}
        renderModel={renderEffortLabel}
        onChange={async (next) => {
          change(next);
          setValue(next);
          return true;
        }}
      />
    );
  }
  render(<EffortPicker />);
  expect(
    screen.getByRole<HTMLButtonElement>("button", {
      name: "Reset effort to High",
    }).disabled,
  ).toBe(true);
  const slider = screen.getByRole("slider", {
    name: "Reasoning effort",
  });
  slider.focus();
  await userEvent.keyboard("{ArrowLeft}");
  expect(change).toHaveBeenLastCalledWith("low");
  await userEvent.click(
    screen.getByRole("button", {
      name: "Reset effort to High",
    }),
  );
  expect(change).toHaveBeenLastCalledWith("high");
  expect(slider.getAttribute("aria-valuetext")).toBe("High");
  expect(
    screen.queryByRole("button", {
      name: /Use .* effort/u,
    }),
  ).toBeNull();
});
it("uses server permission profiles and keeps selected and observed permissions separate", async () => {
  const { SessionControls } = await import("~/src/session-controls");
  localStorage.clear();
  const store = new WorkspaceStore();
  await store.ready();
  store.createThread();
  const policy = {
    revision: 1,
    requested: null,
    status: "effective" as const,
    planned: null,
    plannedProfile: null,
    observed: {
      profile: null,
      native: {
        approvalPolicy: "never",
        sandbox: "read-only",
      },
      evidence: "native_receipt" as const,
      coverage: [],
      limitations: [],
      nativeVersion: null,
    },
    choices: (["ask", "review", "full"] as const).map((profile) => ({
      profile,
      label: {
        ask: "Ask for approval",
        review: "Approve for me",
        full: "Full access",
      }[profile],
      description: "Server-provided permission coverage",
      capability: {
        availability: "available" as const,
        source: "harness" as const,
        evidence: "declared" as const,
      },
      mapping: "equivalent" as const,
      application: "next_turn" as const,
    })),
    controlledSettings: [],
    dispatchBlocked: false,
    reason: null,
    bindingId: "binding",
    nativeSession: null,
  };
  const thread = {
    ...requireFixtureValue(store.active()),
    worktreeId: "/Documents",
    executionPolicy: policy,
    configurations: [
      {
        id: "executionProfile",
        label: "Execution permissions",
        type: "select" as const,
        choices: [],
        application: "next_turn" as const,
      },
    ],
  };
  vi.spyOn(store, "previewControls").mockResolvedValue();
  const configure = vi.spyOn(store, "configureMany").mockResolvedValue();
  const view = render(<SessionControls kind="settings" thread={thread} store={store} connected />);
  await userEvent.click(
    screen.getByRole("button", {
      name: "Change permissions: Custom permissions",
    }),
  );
  expect(
    screen.queryByRole("slider", {
      name: "Reasoning effort",
    }),
  ).toBeNull();
  await waitFor(() => {
    expect(
      screen.getByRole<HTMLButtonElement>("radio", {
        name: "Approve for me",
      }).disabled,
    ).toBe(false);
  });
  await userEvent.click(
    screen.getByRole("radio", {
      name: "Approve for me",
    }),
  );
  expect(configure).toHaveBeenCalledExactlyOnceWith(thread.id, {
    executionProfile: "review",
  });
  view.rerender(
    <SessionControls
      kind="settings"
      thread={{
        ...thread,
        executionPolicy: {
          ...policy,
          requested: "full",
          status: "staged",
        },
      }}
      store={store}
      connected
    />,
  );
  expect(
    screen.getByRole("button", {
      name: "Change permissions: Full access",
    }),
  ).toBeTruthy();
  expect(screen.getByText("Current native permissions: Custom permissions")).toBeTruthy();
  view.rerender(
    <SessionControls
      kind="settings"
      thread={{
        ...thread,
        executionPolicy: {
          ...policy,
          controlledSettings: ["permissionMode"],
          choices: policy.choices.map((choice) =>
            choice.profile === "full"
              ? {
                  ...choice,
                  mapping: "partial",
                  capability: {
                    ...choice.capability,
                    availability: "unsupported",
                    reason: "Native deny rules remain unobserved",
                  },
                }
              : choice,
          ),
        },
        configurations: [
          ...thread.configurations,
          {
            id: "permissionMode",
            label: "Permission mode",
            type: "select",
            value: "default",
            application: "next_turn",
            choices: [
              {
                id: "default",
                label: "Default",
              },
              {
                id: "bypassPermissions",
                label: "Bypass prompts",
              },
            ],
          },
        ],
      }}
      store={store}
      connected
    />,
  );
  expect(
    screen.getByRole<HTMLButtonElement>("radio", {
      name: "Full access",
    }).disabled,
  ).toBe(true);
  expect(screen.getByText("Native deny rules remain unobserved")).toBeTruthy();
  await userEvent.click(screen.getByText("Native permission settings"));
  await userEvent.click(screen.getByRole("button", { name: /Native Permission mode/u }));
  await userEvent.click(screen.getByRole("option", { name: "Bypass prompts" }));
  expect(configure).toHaveBeenLastCalledWith(thread.id, {
    permissionMode: "bypassPermissions",
  });
});
function requireFixtureValue<T>(value: T | null | undefined): T {
  if (value === undefined || value === null) {
    throw new Error("Expected the fixture value to be present.");
  }
  return value;
}

function ConfirmationFixture({ scope, onConfirm }: { scope: string; onConfirm: () => void }) {
  const confirmation = useConfirmation(scope);
  return (
    <>
      <Button
        onClick={() => {
          confirmation.confirm("Discard unsaved work?", onConfirm);
        }}
      >
        Request confirmation
      </Button>
      {confirmation.dialog}
    </>
  );
}
it("runs a confirmed action once and leaves cancelled or escaped work untouched", async () => {
  const accepted = vi.fn<() => void>();
  render(<ConfirmationFixture scope="first" onConfirm={accepted} />);
  await userEvent.click(screen.getByRole("button", { name: "Request confirmation" }));
  await userEvent.click(screen.getByRole("button", { name: "Cancel" }));
  expect(accepted).not.toHaveBeenCalled();
  await userEvent.click(screen.getByRole("button", { name: "Request confirmation" }));
  await userEvent.keyboard("{Escape}");
  expect(accepted).not.toHaveBeenCalled();
  await userEvent.click(screen.getByRole("button", { name: "Request confirmation" }));
  const confirm = screen.getByRole("button", { name: "Confirm" });
  await userEvent.click(confirm);
  fireEvent.click(confirm);
  expect(accepted).toHaveBeenCalledOnce();
  expect(screen.queryByRole("dialog")).toBeNull();
});
it("discards pending confirmations when their conversation changes or unmounts", async () => {
  const accepted = vi.fn<() => void>();
  const view = render(<ConfirmationFixture scope="first" onConfirm={accepted} />);
  await userEvent.click(screen.getByRole("button", { name: "Request confirmation" }));
  const staleConfirm = screen.getByRole("button", { name: "Confirm" });
  view.rerender(<ConfirmationFixture scope="second" onConfirm={accepted} />);
  expect(screen.queryByRole("dialog")).toBeNull();
  fireEvent.click(staleConfirm);
  view.rerender(<ConfirmationFixture scope="first" onConfirm={accepted} />);
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(accepted).not.toHaveBeenCalled();
  await userEvent.click(screen.getByRole("button", { name: "Request confirmation" }));
  const abandoned = screen.getByRole("button", { name: "Confirm" });
  view.unmount();
  fireEvent.click(abandoned);
  expect(accepted).not.toHaveBeenCalled();
});
