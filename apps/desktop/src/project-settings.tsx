import { Button, Disclosure, Input, TextArea, Select, ListBox } from "@swarmie/ui-web";
import { isValidElement, useState, useId } from "react";

import { startAction } from "~/src/action-boundary";
import { useExtensionQuery } from "~/src/components/extension-query";
import { extensionResult } from "~/src/components/extension-result";
import { scalarLabel } from "~/src/components/presentation-values";
import { HarnessSelect } from "~/src/harness-select";
import { isRecord } from "~/src/protocol";
import type { Thread, Project } from "~/src/protocol";
import type { WorkspaceStore } from "~/src/workspace";

export type ProjectDefaults = {
  cwd: string;
  projectId: string;
  revision: number;
  model?: string | null;
  harness?: string | null;
  config?: Record<string, unknown> | null;
};
function nativeDefaultsValue(text: string): Record<string, unknown> | null {
  const value: unknown = JSON.parse(text);
  if (value === null || isRecord(value)) {
    return value;
  }
  throw new TypeError("Native defaults must be a JSON object or null.");
}
export function parseNativeDefaults(text: string): Record<string, unknown> {
  return nativeDefaultsValue(text) ?? {};
}
export function projectWorkspacePaths(project: Project): string[] {
  return [
    ...new Set([
      project.path,
      ...project.worktrees.filter((tree) => !tree.prunable).map((tree) => tree.path),
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
  if (workspace && !projectWorkspacePaths(project).includes(workspace)) {
    throw new Error("Choose an available workspace from this project.");
  }
  const parsedConfig = nativeDefaultsValue(config);
  return {
    projectId: project.id,
    expectedRevision: project.revision ?? 0,
    model: model || null,
    config: parsedConfig,
    directoryPath: workspace || project.path,
    ...(harness === undefined ? {} : { harness: harness || null }),
  };
}
type SettingsEdits = Partial<{ model: string; harness: string; config: string; workspace: string }>;
function projectDefaultFields(defaults: ProjectDefaults | undefined, edits: SettingsEdits) {
  const revision = defaults?.revision ?? 0;
  const defaultModel = edits.model ?? defaults?.model ?? "";
  const defaultHarness = edits.harness ?? defaults?.harness ?? "";
  const defaultConfig = edits.config ?? JSON.stringify(defaults?.config ?? {}, null, 2);
  const defaultWorkspace = edits.workspace ?? defaults?.cwd ?? "";

  return { revision, defaultModel, defaultHarness, defaultConfig, defaultWorkspace };
}
function ProjectSettingsContent({ store, thread }: { store: WorkspaceStore; thread: Thread }) {
  const fieldPrefix = useId();

  const [edits, setEdits] = useState<SettingsEdits>({});
  const [credentialId, setCredentialId] = useState("");
  const [credentialValue, setCredentialValue] = useState("");

  const [busy, setBusy] = useState(false);
  const project = store.getSnapshot().projects.find((item) => item.id === thread.projectId);
  const projectRoot = project?.path ?? thread.worktreeId;
  const workspacePaths = project ? projectWorkspacePaths(project) : [];
  const run = async (action: () => Promise<unknown>) => {
    setBusy(true);
    try {
      await action();
    } catch (error) {
      store.setError(error);
    } finally {
      setBusy(false);
    }
  };
  const fetchDefaults = async () => {
    const [secrets, defaults] = await Promise.all([
      store.invokeExtension<{ credentials: { id: string; configured: boolean }[] }>(
        "resources",
        "credentials.status",
        {},
        extensionResult<{ credentials: { id: string; configured: boolean }[] }>({
          type: "object",
          properties: {
            credentials: {
              type: "array",
              items: {
                type: "object",
                properties: { id: { type: "string" }, configured: { type: "boolean" } },
                required: ["id", "configured"],
              },
            },
          },
          required: ["credentials"],
        }),
      ),
      project
        ? store.invokeExtension<ProjectDefaults>(
            "resources",
            "project.get",
            {
              projectId: project.id,
            },
            extensionResult<ProjectDefaults>({
              type: "object",
              properties: {
                cwd: { type: "string" },
                projectId: { type: "string" },
                revision: { type: "number" },
                model: { anyOf: [{ type: "string" }, { type: "null" }] },
                harness: { anyOf: [{ type: "string" }, { type: "null" }] },
                config: { anyOf: [{ type: "object", additionalProperties: {} }, { type: "null" }] },
              },
              required: ["cwd", "projectId", "revision"],
            }),
          )
        : Promise.resolve(undefined),
    ]);
    return { credentials: secrets.credentials, defaults };
  };
  const catalog = useExtensionQuery(
    store,
    ["project-settings", projectRoot, project?.id],
    fetchDefaults,
  );
  const credentials = catalog.data?.credentials ?? [];
  const defaults = catalog.data?.defaults;
  const { revision, defaultModel, defaultHarness, defaultConfig, defaultWorkspace } =
    projectDefaultFields(defaults, edits);
  const setDefaultModel = (model: string) => {
    setEdits((previous) => ({ ...previous, model }));
  };
  const setDefaultHarness = (harness: string) => {
    setEdits((previous) => ({ ...previous, harness }));
  };
  const setDefaultConfig = (config: string) => {
    setEdits((previous) => ({ ...previous, config }));
  };
  const setDefaultWorkspace = (workspace: string) => {
    setEdits((previous) => ({ ...previous, workspace }));
  };
  const load = async () => {
    await catalog.refresh();
    setEdits({});
  };
  const selectedHarness = defaultHarness || thread.harness;
  let nativeDefaults: Record<string, unknown> = {};
  try {
    nativeDefaults = parseNativeDefaults(defaultConfig);
  } catch {
    /* Preserve incomplete edits. */
  }
  return (
    <div className="panel-body">
      <h3>Project defaults</h3>
      <label htmlFor={`${fieldPrefix}-field-1`}>
        Agent tool override
        <HarnessSelect
          id={`${fieldPrefix}-field-1`}
          label="Agent tool override"
          value={defaultHarness}
          onChange={setDefaultHarness}
          inherited="Use conversation agent"
          harnesses={store.getSnapshot().harnesses}
          disabledHarnesses={store.getSnapshot().disabledHarnesses}
        />
      </label>
      <label htmlFor={`${fieldPrefix}-field-2`}>
        Model override
        <Input
          id={`${fieldPrefix}-field-2`}
          value={defaultModel}
          onChange={(e) => {
            setDefaultModel(e.target.value);
          }}
        />
      </label>
      <label htmlFor={`${fieldPrefix}-field-3`}>
        Default workspace
        <Select
          aria-label="Default workspace"
          value={defaultWorkspace}
          onChange={(key) => {
            if (typeof key === "string") {
              setDefaultWorkspace(key);
            }
          }}
          placeholder="Select…"
        >
          <Select.Trigger id={`${fieldPrefix}-field-3`}>
            <Select.Value />
            <Select.Indicator />
          </Select.Trigger>
          <Select.Popover>
            <ListBox aria-label="Default workspace">
              {[
                { id: "", label: "Project folder" },
                ...(defaultWorkspace && !workspacePaths.includes(defaultWorkspace)
                  ? [
                      {
                        id: defaultWorkspace,
                        label: `Unavailable: ${defaultWorkspace}`,
                        disabled: true,
                      },
                    ]
                  : []),
                ...workspacePaths.map((path) => ({
                  id: path,
                  label: project?.worktrees.find((tree) => tree.path === path)?.branch ?? path,
                })),
              ].map((choice) => (
                <ListBox.Item
                  key={choice.id}
                  id={choice.id}
                  textValue={choice.label}
                  isDisabled={"disabled" in choice && choice.disabled === true}
                >
                  {"icon" in choice && isValidElement(choice.icon) ? choice.icon : null}
                  {choice.label}
                  <ListBox.ItemIndicator />
                </ListBox.Item>
              ))}
            </ListBox>
          </Select.Popover>
        </Select>
      </label>
      <p className="panel-note">
        Used for new conversations unless another workspace is selected. Choose an existing project
        folder or worktree.
      </p>
      <label htmlFor={`${fieldPrefix}-field-4`}>
        Native controls and permissions JSON
        <TextArea
          id={`${fieldPrefix}-field-4`}
          value={defaultConfig}
          onChange={(event) => {
            setDefaultConfig(event.target.value);
          }}
        />
      </label>
      {selectedHarness === thread.harness &&
        (thread.configurations ?? [])
          .filter(
            (option) =>
              (option.choices?.length ?? 0) > 0 &&
              (option.id === "mode" ||
                /permission|approval|sandbox/iu.test(`${option.id} ${option.label}`)),
          )
          .map((option) => (
            <label htmlFor={`${fieldPrefix}-field-5-${option.id}`} key={option.id}>
              {option.label}
              <Select
                aria-label={option.label}
                value={scalarLabel(nativeDefaults[option.id], "")}
                onChange={(key) => {
                  if (typeof key === "string") {
                    ((value) => {
                      try {
                        setDefaultConfig(
                          JSON.stringify(
                            { ...parseNativeDefaults(defaultConfig), [option.id]: value || null },
                            null,
                            2,
                          ),
                        );
                      } catch (error) {
                        store.setError(error);
                      }
                    })(key);
                  }
                }}
                placeholder="Select…"
              >
                <Select.Trigger id={`${fieldPrefix}-field-5-${option.id}`}>
                  <Select.Value />
                  <Select.Indicator />
                </Select.Trigger>
                <Select.Popover>
                  <ListBox aria-label={option.label}>
                    {[
                      { id: "", label: "Clear value (null)" },
                      ...(option.choices ?? []).map((choice) => ({
                        id: choice.id,
                        label: choice.label,
                      })),
                    ].map((choice) => (
                      <ListBox.Item
                        key={choice.id}
                        id={choice.id}
                        textValue={choice.label}
                        isDisabled={"disabled" in choice && choice.disabled === true}
                      >
                        {"icon" in choice && isValidElement(choice.icon) ? choice.icon : null}
                        {choice.label}
                        <ListBox.ItemIndicator />
                      </ListBox.Item>
                    ))}
                  </ListBox>
                </Select.Popover>
              </Select>
            </label>
          ))}
      <Button
        variant="tertiary"
        size="sm"
        disabled={selectedHarness !== thread.harness}
        onClick={() => {
          setDefaultConfig(JSON.stringify(thread.config ?? {}, null, 2));
        }}
      >
        Use conversation controls
      </Button>
      <p className="panel-note">
        Nonsecret project controls merge before conversation overrides. Configure authentication in
        the native harness. Null clears saved defaults.
      </p>
      <Button
        variant="tertiary"
        size="sm"
        disabled={busy || !project}
        onClick={() => {
          startAction(
            run(async () => {
              if (!project) {
                throw new Error("Open a project before saving defaults.");
              }
              await store.invokeExtension(
                "resources",
                "project.set",
                projectDefaultsPayload(
                  { ...project, revision },
                  defaultModel,
                  defaultConfig,
                  defaultWorkspace,
                  defaultHarness,
                ),
              );
              await store.refreshProject(project.id);
              await load();
            }),
            store.setError.bind(store),
          );
        }}
      >
        Save project defaults
      </Button>
      <hr />
      <Disclosure>
        <Disclosure.Heading>
          <Button slot="trigger" variant="tertiary">
            <span>Credentials</span>
            <Disclosure.Indicator />
          </Button>
        </Disclosure.Heading>
        <Disclosure.Content>
          <Disclosure.Body>
            <p className="panel-note">
              Values stay in the daemon’s private local credential store. They are never returned to
              this interface.
            </p>
            {credentials.map((credential) => (
              <div className="resource-row" key={credential.id}>
                <span>{credential.id} · configured</span>
                <Button
                  variant="tertiary"
                  size="sm"
                  onClick={() => {
                    startAction(
                      run(async () => {
                        await store.invokeExtension("resources", "credentials.remove", {
                          id: credential.id,
                        });
                        await load();
                      }),
                      store.setError.bind(store),
                    );
                  }}
                >
                  Remove
                </Button>
              </div>
            ))}
            <label htmlFor={`${fieldPrefix}-field-6`}>
              Credential ID
              <Input
                id={`${fieldPrefix}-field-6`}
                value={credentialId}
                autoComplete="off"
                onChange={(e) => {
                  setCredentialId(e.target.value);
                }}
              />
            </label>
            <label htmlFor={`${fieldPrefix}-field-7`}>
              Secret value
              <Input
                id={`${fieldPrefix}-field-7`}
                type="password"
                autoComplete="off"
                value={credentialValue}
                onChange={(e) => {
                  setCredentialValue(e.target.value);
                }}
              />
            </label>
            <Button
              variant="tertiary"
              size="sm"
              disabled={busy || !credentialId || !credentialValue}
              onClick={() => {
                startAction(
                  run(async () => {
                    await store.invokeExtension("resources", "credentials.set", {
                      id: credentialId,
                      value: credentialValue,
                    });
                    setCredentialValue("");
                    await load();
                  }),
                  store.setError.bind(store),
                );
              }}
            >
              Store credential
            </Button>
          </Disclosure.Body>
        </Disclosure.Content>
      </Disclosure>
    </div>
  );
}

export function ProjectSettings(props: { store: WorkspaceStore; thread: Thread }) {
  return (
    <ProjectSettingsContent
      key={`${props.thread.projectId}:${props.thread.worktreeId}`}
      {...props}
    />
  );
}
