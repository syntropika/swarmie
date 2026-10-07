// @vitest-environment happy-dom
import { afterEach, expect, it, vi } from "vitest";
import { cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ProjectDialog } from "./ProjectDialog";
import { WorkspaceStore } from "./workspace";

afterEach(() => { cleanup(); vi.restoreAllMocks(); });

async function setup() {
  localStorage.clear();
  const store = new WorkspaceStore();
  await store.ready();
  store.getSnapshot().connected = true;
  store.getSnapshot().extensions = [{ id: "workspace" }];
  const close = vi.fn();
  const open = vi.spyOn(store, "openProject").mockResolvedValue({ id: "/project", name: "Project", path: "/project", worktrees: [] });
  render(<ProjectDialog store={store} onClose={close} />);
  return { store, close, open };
}

it("requires a source folder and opens it once with the chosen display name", async () => {
  const { open, close } = await setup();
  const create = screen.getByRole("button", { name: "Create project" }) as HTMLButtonElement;
  expect(create.disabled).toBe(true);
  await userEvent.type(screen.getByRole("textbox", { name: "Project name" }), "My workspace");
  await userEvent.click(screen.getByRole("button", { name: "Add" }));
  await userEvent.type(screen.getByRole("textbox", { name: "Folder path" }), " /project ");
  await userEvent.click(screen.getByRole("button", { name: "Add folder" }));
  expect(create.disabled).toBe(false);
  await userEvent.click(create);
  await waitFor(() => expect(open).toHaveBeenCalledOnce());
  expect(open).toHaveBeenCalledWith("/project", "My workspace");
  expect(close).toHaveBeenCalledOnce();
});

it("keeps a failed project open with an actionable inline error", async () => {
  const { store, open, close } = await setup();
  open.mockImplementation(async () => { store.setError("Folder does not exist."); return undefined; });
  await userEvent.click(screen.getByRole("button", { name: "Add" }));
  await userEvent.type(screen.getByRole("textbox", { name: "Folder path" }), "/missing{Enter}");
  await userEvent.click(screen.getByRole("button", { name: "Create project" }));
  expect(await screen.findByRole("alert")).toHaveProperty("textContent", "Folder does not exist.");
  expect(close).not.toHaveBeenCalled();
});

it("retains cloning through the source menu and opens the returned path", async () => {
  const { store, open } = await setup();
  const invoke = vi.spyOn(store, "invokeExtension").mockResolvedValue({ path: "/parent/project" });
  await userEvent.type(screen.getByRole("textbox", { name: "Project name" }), "project");
  await userEvent.click(screen.getByRole("button", { name: "Add a folder on this computer" }));
  await userEvent.click(screen.getByRole("menuitem", { name: "Clone a repository" }));
  await userEvent.type(screen.getByRole("textbox", { name: "Parent folder" }), "/parent");
  await userEvent.type(screen.getByRole("textbox", { name: "Repository URL" }), "https://example.com/project.git");
  await userEvent.click(screen.getByRole("button", { name: "Create project" }));
  await waitFor(() => expect(open).toHaveBeenCalledWith("/parent/project", "project"));
  expect(invoke).toHaveBeenCalledWith("workspace", "project.clone", { parent: "/parent", name: "project", url: "https://example.com/project.git", initializeGit: true });
});

it("lets Cancel dismiss without opening or creating anything", async () => {
  const { open, close } = await setup();
  await userEvent.click(screen.getByRole("button", { name: "Cancel" }));
  expect(close).toHaveBeenCalledOnce();
  expect(open).not.toHaveBeenCalled();
});
