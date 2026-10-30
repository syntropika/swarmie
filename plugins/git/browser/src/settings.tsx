import { Button, Input, Label, TextField, TextArea } from "@swarmie/ui-web";
import { useState } from "react";

import type { GitSettingsValue, SetupScript } from "./protocol";
import { RecoveryNotice } from "./recovery-notice";
import { useCheckout } from "./use-checkout";
import type { GitViewProps } from "./view";
import { Empty } from "./workspace";

type SettingsState = ReturnType<typeof useCheckout>;
function defaultSetup(platform: string): SetupScript {
  if (platform === "windows") {
    return {
      enabled: false,
      shell: "pwsh",
      arguments: ["-NoProfile", "-Command"],
      script: "",
      working_directory: ".",
    };
  }
  if (platform === "linux" || platform === "macos") {
    return {
      enabled: false,
      shell: "/bin/sh",
      arguments: ["-c"],
      script: "",
      working_directory: ".",
    };
  }
  return { enabled: false, shell: "", arguments: [], script: "", working_directory: "." };
}
export function GitSettings({
  connection,
  path,
  projectId,
}: Omit<GitViewProps, "onWorktreeCreated">) {
  const state = useCheckout(connection, path, projectId);
  const [draft, setDraft] = useState<GitSettingsValue | null>(null);
  if (connection === null && state.settings === null) {
    return (
      <Empty
        title="Connect an execution node"
        detail="Git settings belong to the project and the node that owns its checkout."
      />
    );
  }
  if (state.error !== null && state.settings === null) {
    return (
      <p role="alert" className="p-6 text-sm text-danger">
        {state.error}
      </p>
    );
  }
  if (state.settings === null || state.platform === null) {
    return <output className="p-6 text-sm text-muted">Loading Git settings…</output>;
  }
  return (
    <div className="h-full overflow-auto p-6">
      <h2 className="text-lg font-medium">Git</h2>
      <p className="mt-2 mb-6 text-sm text-muted">
        Preferences for this project on {connection?.nodeId ?? "the disconnected node"}.
      </p>
      {state.error === null ? null : (
        <p role="alert" className="mb-4 text-sm text-danger">
          {state.error}
        </p>
      )}
      <SettingsForm
        initial={state.settings}
        platform={state.platform}
        state={state}
        projectId={projectId}
        settings={draft ?? state.settings}
        setSettings={setDraft}
      />
    </div>
  );
}

function SettingsForm({
  initial,
  platform,
  state,
  projectId,
  settings,
  setSettings,
}: {
  initial: GitSettingsValue;
  platform: string;
  state: SettingsState;
  projectId: string | undefined;
  settings: GitSettingsValue;
  setSettings: (settings: GitSettingsValue) => void;
}) {
  const [tab, setTab] = useState<"general" | "setup">("general");
  const [saved, setSaved] = useState(false);
  const disabled = state.busy || !state.fresh || state.pendingOperation !== null;
  const dirty = JSON.stringify(initial) !== JSON.stringify(settings);
  const script = settings.setup_by_platform[platform] ?? defaultSetup(platform);
  function updateScript(update: Partial<SetupScript>) {
    setSettings({
      ...settings,
      setup_by_platform: { ...settings.setup_by_platform, [platform]: { ...script, ...update } },
    });
  }
  function save() {
    state
      .managed({
        kind: "settings_set",
        ...(projectId === undefined ? {} : { project_id: projectId }),
        settings,
      })
      .then((result) => {
        setSaved(result?.kind === "settings");
      })
      .catch(state.fail);
  }
  return (
    <div className="grid max-w-2xl gap-5">
      <div className="flex gap-1" aria-label="Git settings section">
        <Button
          size="sm"
          variant={tab === "general" ? "secondary" : "tertiary"}
          onPress={() => {
            setTab("general");
          }}
        >
          General
        </Button>
        <Button
          size="sm"
          variant={tab === "setup" ? "secondary" : "tertiary"}
          onPress={() => {
            setTab("setup");
          }}
        >
          Setup
        </Button>
      </div>
      {tab === "general" ? (
        <GeneralSettings settings={settings} change={setSettings} />
      ) : (
        <div className="grid gap-4">
          <h3 className="text-sm font-medium">Preparation script · {platform}</h3>
          <p className="text-sm leading-6 text-muted">
            Runs on this execution system after a new worktree is created. A failure keeps the
            checkout available for inspection.
          </p>
          <label className="flex items-center gap-2 text-sm">
            <input
              type="checkbox"
              checked={script.enabled}
              onChange={(event) => {
                updateScript({ enabled: event.target.checked });
              }}
            />
            Run preparation after creating a worktree
          </label>
          <SettingsTextField
            label="Shell executable"
            value={script.shell}
            change={(shell) => {
              updateScript({ shell });
            }}
          />
          <TextField>
            <Label>Shell arguments</Label>
            <TextArea
              rows={2}
              value={script.arguments.join("\n")}
              onChange={(event) => {
                updateScript({
                  arguments: event.target.value
                    .split("\n")
                    .filter((argument) => argument.length > 0),
                });
              }}
            />
            <span className="text-xs text-muted">
              One argument per line. The script is passed as the final argument.
            </span>
          </TextField>
          <SettingsTextField
            label="Working directory relative to the new checkout"
            value={script.working_directory}
            change={(working_directory) => {
              updateScript({ working_directory });
            }}
          />
          <TextField>
            <Label>Preparation script</Label>
            <TextArea
              rows={8}
              value={script.script}
              onChange={(event) => {
                updateScript({ script: event.target.value });
              }}
              spellCheck={false}
            />
          </TextField>
        </div>
      )}
      <div className="flex items-center gap-3 border-t border-border pt-4">
        <Button size="sm" isDisabled={disabled || !dirty} onPress={save}>
          Save Git settings
        </Button>
        {saved ? <output className="text-xs text-muted">Settings saved.</output> : null}
      </div>
      <RecoveryNotice key={state.pendingOperation} state={state} />
    </div>
  );
}

function SettingsTextField({
  label,
  value,
  change,
}: {
  label: string;
  value: string;
  change: (value: string) => void;
}) {
  return (
    <TextField>
      <Label>{label}</Label>
      <Input
        value={value}
        onChange={(event) => {
          change(event.target.value);
        }}
      />
    </TextField>
  );
}

function GeneralSettings({
  settings,
  change,
}: {
  settings: GitSettingsValue;
  change: (settings: GitSettingsValue) => void;
}) {
  return (
    <div className="grid gap-5">
      <label className="grid gap-2 text-sm">
        Default diff layout
        <select
          value={settings.diff_layout}
          onChange={(event) => {
            change({
              ...settings,
              diff_layout: event.target.value === "split" ? "split" : "unified",
            });
          }}
          className="h-10 rounded-lg border border-border bg-default px-3 text-sm"
        >
          <option value="unified">Unified</option>
          <option value="split">Split</option>
        </select>
      </label>
      <label className="flex items-center gap-2 text-sm">
        <input
          type="checkbox"
          checked={settings.include_local_changes}
          onChange={(event) => {
            change({ ...settings, include_local_changes: event.target.checked });
          }}
        />
        Offer to include selected local changes in new worktrees
      </label>
      <SettingsTextField
        label="Managed worktree directory on this node"
        value={settings.managed_root ?? ""}
        change={(root) => {
          change({ ...settings, managed_root: root.trim() === "" ? null : root });
        }}
      />
      <SettingsTextField
        label="Preferred branch prefix"
        value={settings.branch_prefix}
        change={(branch_prefix) => {
          change({ ...settings, branch_prefix });
        }}
      />
      <label className="flex items-center gap-2 text-sm">
        <input
          type="checkbox"
          checked={settings.run_commit_hooks}
          onChange={(event) => {
            change({ ...settings, run_commit_hooks: event.target.checked });
          }}
        />
        Run Git hooks on commit
      </label>
      <p className="text-xs leading-5 text-muted">
        Off by default. Applies to commits prepared here. Commit signing follows the configured Git
        settings on the execution node.
      </p>
      <TextField>
        <Label>Commit message guidance</Label>
        <TextArea
          rows={3}
          value={settings.commit_guidance}
          onChange={(event) => {
            change({ ...settings, commit_guidance: event.target.value });
          }}
        />
      </TextField>
      <div className="grid gap-3 border-t border-border pt-4">
        <label className="flex items-center gap-2 text-sm">
          <input
            type="checkbox"
            checked={settings.cleanup_enabled}
            onChange={(event) => {
              change({ ...settings, cleanup_enabled: event.target.checked });
            }}
          />
          Request cleanup when recovery and lifecycle checks allow it
        </label>
        <TextField>
          <Label>Worktrees to retain</Label>
          <Input
            type="number"
            min={0}
            value={settings.retained_worktrees ?? ""}
            onChange={(event) => {
              const count = event.target.value === "" ? null : Number(event.target.value);
              if (count === null || (Number.isSafeInteger(count) && count >= 0)) {
                change({ ...settings, retained_worktrees: count });
              }
            }}
          />
          <span className="text-xs leading-5 text-muted">
            An empty value leaves retention unconfigured. Cleanup stays blocked until the execution
            host qualifies restoration and stops every writer.
          </span>
        </TextField>
      </div>
    </div>
  );
}
