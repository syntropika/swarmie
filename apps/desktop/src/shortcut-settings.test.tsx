// @vitest-environment happy-dom
import { cleanup, fireEvent, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { afterEach, expect, it, vi } from "vitest";

import { ShortcutSettings } from "~/src/shortcut-settings";
import { defaultShortcuts } from "~/src/shortcuts";
import type { Shortcuts } from "~/src/shortcuts";
import { renderWithQuery as render } from "~/src/test-fixtures";

afterEach(() => {
  cleanup();
  localStorage.clear();
  vi.restoreAllMocks();
});

function Fixture({ initial = defaultShortcuts }: { initial?: Shortcuts }) {
  const [shortcuts, setShortcuts] = useState(initial);
  return <ShortcutSettings shortcuts={shortcuts} onChange={setShortcuts} />;
}

async function editPalette() {
  await userEvent.click(screen.getByRole("button", { name: "Edit shortcut for Command palette" }));
  return await screen.findByRole("dialog", { name: "Record shortcut for Command palette" });
}

it("records a combination without triggering workspace shortcuts and resets only its row", async () => {
  const globalKey = vi.fn<(event: KeyboardEvent) => void>();
  window.addEventListener("keydown", globalKey);
  try {
    render(<Fixture initial={{ ...defaultShortcuts, files: "Mod+O" }} />);
    expect(screen.queryByRole("textbox")).toBeNull();
    const recorder = await editPalette();
    await waitFor(() => {
      expect(document.activeElement).toBe(recorder);
    });
    fireEvent.keyDown(recorder, { key: "j", ctrlKey: true, shiftKey: true });
    expect(globalKey).not.toHaveBeenCalled();
    expect(JSON.parse(localStorage.getItem("swarmie.shortcuts") ?? "{}")).toEqual({
      ...defaultShortcuts,
      files: "Mod+O",
      palette: "Mod+Shift+J",
    });
    expect(
      screen.queryByRole("dialog", { name: "Record shortcut for Command palette" }),
    ).toBeNull();
    await waitFor(() => {
      expect(
        screen
          .getByRole("button", { name: "Edit shortcut for Command palette" })
          .closest("td")
          ?.contains(document.activeElement),
      ).toBe(true);
    });
    await userEvent.click(
      screen.getByRole("button", { name: "Reset shortcut for Command palette" }),
    );
    expect(JSON.parse(localStorage.getItem("swarmie.shortcuts") ?? "{}")).toEqual({
      ...defaultShortcuts,
      files: "Mod+O",
    });
  } finally {
    window.removeEventListener("keydown", globalKey);
  }
});

it("rejects conflicts and invalid keys, and cancels recording without saving", async () => {
  render(<Fixture />);
  const recorder = await editPalette();
  fireEvent.keyDown(recorder, { key: "Shift", shiftKey: true });
  expect(screen.queryByRole("alert")).toBeNull();
  fireEvent.keyDown(recorder, { key: "n", ctrlKey: true });
  expect(screen.getByRole("alert").textContent).toContain("not assigned to another action");
  fireEvent.keyDown(recorder, { key: "j" });
  expect(screen.getByRole("alert").textContent).toContain("Use Command, Control or Alt");
  fireEvent.keyDown(recorder, { key: "Escape" });
  expect(localStorage.getItem("swarmie.shortcuts")).toBeNull();
  expect(screen.queryByRole("alert")).toBeNull();
  await waitFor(() => {
    expect(
      screen
        .getByRole("button", { name: "Edit shortcut for Command palette" })
        .closest("td")
        ?.contains(document.activeElement),
    ).toBe(true);
  });
  await editPalette();
  await userEvent.click(screen.getByRole("button", { name: "Cancel" }));
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(localStorage.getItem("swarmie.shortcuts")).toBeNull();
});

it("prevents a row reset from taking another action's binding and offers reset all", async () => {
  render(<Fixture initial={{ ...defaultShortcuts, palette: "Mod+J", files: "Mod+K" }} />);
  await userEvent.click(screen.getByRole("button", { name: "Reset shortcut for Command palette" }));
  expect(screen.getByRole("alert").textContent).toContain("not assigned to another action");
  expect(localStorage.getItem("swarmie.shortcuts")).toBeNull();
  await userEvent.click(screen.getByRole("button", { name: "Reset all shortcuts" }));
  expect(screen.queryByRole("alert")).toBeNull();
  expect(
    screen.getByRole<HTMLButtonElement>("button", { name: "Reset shortcut for Command palette" })
      .disabled,
  ).toBe(true);
  expect(
    screen.getByRole<HTMLButtonElement>("button", { name: "Reset shortcut for Find file" })
      .disabled,
  ).toBe(true);
});

it("keeps the current binding when storage fails", async () => {
  render(<Fixture />);
  const recorder = await editPalette();
  vi.spyOn(localStorage, "setItem").mockImplementation(() => {
    throw new Error("Storage unavailable");
  });
  fireEvent.keyDown(recorder, { key: "j", ctrlKey: true });
  expect(screen.getByRole("alert").textContent).toBe("Storage unavailable");
  fireEvent.keyDown(recorder, { key: "Escape" });
  expect(
    screen.getByRole<HTMLButtonElement>("button", { name: "Reset shortcut for Command palette" })
      .disabled,
  ).toBe(true);
});
