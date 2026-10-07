import { useEffect, useState } from "react";
import { Disclosure } from "@/components/ui/disclosure";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { Select } from "@/components/ui/select";
import { HarnessSelect } from "./HarnessSelect";
import type { Thread, Project } from "./protocol";
import type { WorkspaceStore } from "./workspace";
export type ProjectDefaults = {
  cwd: string;
  model?: string | null;
  harness?: string | null;
  config?: Record<string, unknown> | null;
  workspace?: string | null;
};
export function parseNativeDefaults(text: string): Record<string, unknown> {
  const value: unknown = JSON.parse(text);
  if (value === null) return {};
  if (typeof value !== "object" || Array.isArray(value))
    throw new Error("Native defaults must be a JSON object or null.");
  return value as Record<string, unknown>;
}
export function projectWorkspacePaths(project: Project): string[] {
  return [
    ...new Set([
      project.path,
      ...project.worktrees
        .filter((tree) => !tree.prunable)
        .map((tree) => tree.path),
    ]),
  ];
}
export function projectDefaultsPayload(
  project: Project,
  model: string,
  config: string,
  workspace: string,
  harness?: string,
) {
  if (workspace && !projectWorkspacePaths(project).includes(workspace))
    throw new Error("Choose an available workspace from this project.");
  parseNativeDefaults(config);
  return {
    cwd: project.path,
    model: model || null,
    config: JSON.parse(config) as Record<string, unknown> | null,
    workspace: workspace || null,
    ...(harness !== undefined ? { harness: harness || null } : {}),
  };
}
export function ProjectSettings({ store, thread }: { store: WorkspaceStore; thread: Thread }) {
  const [defaultModel, setDefaultModel] = useState("");
  const [defaultHarness, setDefaultHarness] = useState("");
  const [defaultConfig, setDefaultConfig] = useState("{}");
  const [defaultWorkspace, setDefaultWorkspace] = useState("");
  const [credentialId, setCredentialId] = useState("");
  const [credentialValue, setCredentialValue] = useState("");
  const [credentials, setCredentials] = useState<{ id: string; configured: boolean }[]>([]);
  const [busy, setBusy] = useState(false);
  const project = store.getSnapshot().projects.find((item) => item.id === thread.projectId);
  const projectRoot = project?.path ?? thread.worktreeId;
  const workspacePaths = project ? projectWorkspacePaths(project) : [];
  const selectedHarness = defaultHarness || thread.harness;
  let nativeDefaults: Record<string, unknown> = {};
  try { nativeDefaults = parseNativeDefaults(defaultConfig); } catch { /* Preserve incomplete edits. */ }
  const run = async (action: () => Promise<unknown>) => {
    setBusy(true);
    try { await action(); } catch (error) { store.setError(error); } finally { setBusy(false); }
  };
  const load = async () => {
    const [secrets, defaults] = await Promise.all([
      store.invokeExtension<{ credentials: typeof credentials }>("resources", "credentials.status", {}),
      projectRoot ? store.invokeExtension<ProjectDefaults>("resources", "project.get", { cwd: projectRoot }) : Promise.resolve({} as ProjectDefaults),
    ]);
    setCredentials(secrets.credentials);
    setDefaultModel(defaults.model ?? "");
    setDefaultHarness(defaults.harness ?? "");
    setDefaultConfig(JSON.stringify(defaults.config ?? {}, null, 2));
    setDefaultWorkspace(defaults.workspace ?? "");
  };
  useEffect(() => { void run(load); }, [projectRoot]);
  return (
    <div className="panel-body">
      <h3>Project defaults</h3>
      <label>
        Harness override
        <HarnessSelect label="Harness override" value={defaultHarness} onChange={setDefaultHarness}
          inherited="Use conversation harness" harnesses={store.getSnapshot().harnesses} />
      </label>
      <label>
        Model override
        <Input
          value={defaultModel}
          onChange={(e) => setDefaultModel(e.target.value)}
        />
      </label>
      <label>
        Default workspace
        <Select label="Default workspace" value={defaultWorkspace} onChange={setDefaultWorkspace}
          options={[{ id: "", label: "Project folder" }, ...(defaultWorkspace && !workspacePaths.includes(defaultWorkspace) ? [{ id: defaultWorkspace, label: `Unavailable: ${defaultWorkspace}`, disabled: true }] : []), ...workspacePaths.map((path) => ({ id: path, label: project?.worktrees.find((tree) => tree.path === path)?.branch ?? path }))]} />
      </label>
      <p className="panel-note">
        Used for new conversations unless another workspace is selected. Choose
        an existing project folder or worktree.
      </p>
      <label>
        Native controls and permissions JSON
        <Textarea
          value={defaultConfig}
          onChange={(event) => setDefaultConfig(event.target.value)}
        />
      </label>
      {selectedHarness === thread.harness &&
        (thread.configurations ?? [])
          .filter(
            (option) =>
              option.choices?.length &&
              (option.id === "mode" ||
                /permission|approval|sandbox/i.test(
                  `${option.id} ${option.label}`,
                )),
          )
          .map((option) => (
            <label key={option.id}>
              {option.label}
              <Select label={option.label} value={String(nativeDefaults[option.id] ?? "")}
                options={[{ id: "", label: "Clear value (null)" }, ...option.choices!.map((choice) => ({ id: choice.id, label: choice.label }))]}
                onChange={(value) => {
                  try { setDefaultConfig(JSON.stringify({ ...parseNativeDefaults(defaultConfig), [option.id]: value || null }, null, 2)); }
                  catch (error) { store.setError(error); }
                }} />
            </label>
          ))}
      <Button
        variant="ghost"
        className="small-button"
        disabled={selectedHarness !== thread.harness}
        onClick={() =>
          setDefaultConfig(JSON.stringify(thread.config ?? {}, null, 2))
        }
      >
        Use conversation controls
      </Button>
      <p className="panel-note">
        Nonsecret project controls merge before conversation overrides. Configure
        authentication in the native harness. Null clears saved defaults.
      </p>
      <Button
        variant="ghost"
        className="small-button"
        disabled={busy || !project}
        onClick={() =>
          void run(async () => {
            if (!project)
              throw new Error("Open a project before saving defaults.");
            await store.invokeExtension(
              "resources",
              "project.set",
              projectDefaultsPayload(
                project,
                defaultModel,
                defaultConfig,
                defaultWorkspace,
                defaultHarness,
              ),
            );
            await load();
          })
        }
      >
        Save project defaults
      </Button>
      <hr />
      <Disclosure>
        <Disclosure.Summary>Credentials</Disclosure.Summary>
        <p className="panel-note">
          Values stay in the daemon's private local credential store. They are
          never returned to this interface.
        </p>
        {credentials.map((credential) => (
          <div className="resource-row" key={credential.id}>
            <span>{credential.id} · configured</span>
            <Button
              variant="ghost"
              className="small-button"
              onClick={() =>
                void run(async () => {
                  await store.invokeExtension(
                    "resources",
                    "credentials.remove",
                    { id: credential.id },
                  );
                  await load();
                })
              }
            >
              Remove
            </Button>
          </div>
        ))}
        <label>
          Credential ID
          <Input
            value={credentialId}
            autoComplete="off"
            onChange={(e) => setCredentialId(e.target.value)}
          />
        </label>
        <label>
          Secret value
          <Input
            type="password"
            autoComplete="off"
            value={credentialValue}
            onChange={(e) => setCredentialValue(e.target.value)}
          />
        </label>
        <Button
          variant="ghost"
          className="small-button"
          disabled={busy || !credentialId || !credentialValue}
          onClick={() =>
            void run(async () => {
              await store.invokeExtension("resources", "credentials.set", {
                id: credentialId,
                value: credentialValue,
              });
              setCredentialValue("");
              await load();
            })
          }
        >
          Store credential
        </Button>
      </Disclosure>
    </div>
  );
}
