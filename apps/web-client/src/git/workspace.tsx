import { useSource } from "@swarmie/client-ui";
import { ConnectionPhase } from "@swarmie/plugin-browser/connections";
import type { CapabilityTab } from "@swarmie/plugin-browser/views";
import { GitSettings, GitView } from "@swarmie/plugin-git-browser";
import { Button } from "@swarmie/ui-web";
import { useRef, useState } from "react";

import { useClient } from "../client/context";
import {
  isDesktopNode,
  desktopDirectorySelectionAvailable,
  selectDesktopGitDirectory,
} from "../desktop-node";
import { useGitConnection } from "./connection";
import { gitDescriptor, gitTarget } from "./targets";
import type { GitTarget } from "./targets";

export function GitTargetChooser({ onChoose }: { onChoose: (target: GitTarget) => void }) {
  const { connectionState, projectsState, gitPresentation } = useClient();
  const nodes = useSource(connectionState);
  const git = useSource(gitPresentation.state);
  const availableNodes = nodes.nodes.filter((entry) => {
    const capability = git.nodes[entry.id];
    return (
      git.enabled &&
      nodes.sessions[entry.id]?.phase === ConnectionPhase.Connected &&
      capability?.available === true &&
      capability.principal === entry.principal &&
      capability.generation === nodes.sessions[entry.id]?.generation
    );
  });
  const projects = useSource(projectsState);
  const [nodeId, setNodeId] = useState(
    availableNodes.find((entry) => entry.id === nodes.selected)?.id ?? availableNodes[0]?.id ?? "",
  );
  const [path, setPath] = useState("");
  const [projectId, setProjectId] = useState<string | undefined>();
  const [pickerError, setPickerError] = useState<string | null>(null);
  const [picking, setPicking] = useState(false);
  const selectionVersion = useRef(0);
  const node = availableNodes.find((entry) => entry.id === nodeId);
  const folders = projects.personalFolders.filter(
    (entry) =>
      entry.nodeId === nodeId &&
      projects.records.some((project) => project.id === entry.projectId && !project.archived),
  );
  function pickerFailed(error: unknown) {
    setPickerError(error instanceof Error ? error.message : String(error));
  }
  async function chooseDirectory() {
    const version = selectionVersion.current;
    setPicking(true);
    setPickerError(null);
    try {
      const selected = await selectDesktopGitDirectory();
      if (selected !== null && selected !== "" && version === selectionVersion.current) {
        setPath(selected);
        setProjectId(undefined);
      }
    } catch (error: unknown) {
      setPickerError(error instanceof Error ? error.message : String(error));
    } finally {
      setPicking(false);
    }
  }
  return (
    <form
      className="grid gap-4"
      onSubmit={(event) => {
        event.preventDefault();
        if (node && path.trim() !== "") {
          onChoose({
            nodeId: node.id,
            principal: node.principal,
            path: path.trim(),
            ...(projectId !== undefined && projectId !== "" ? { projectId } : {}),
          });
        }
      }}
    >
      <label className="grid gap-2 text-xs">
        Execution node
        <select
          value={node?.id ?? ""}
          onChange={(event) => {
            selectionVersion.current += 1;
            setNodeId(event.target.value);
            setPath("");
            setProjectId(undefined);
          }}
          className="rounded-lg border border-border bg-default px-3 py-2 text-sm"
        >
          <option value="">Choose a node with Git support</option>
          <option value="" disabled>
            Choose a node with Git support
          </option>
          {availableNodes.map((entry) => (
            <option key={entry.id} value={entry.id}>
              {entry.label}
            </option>
          ))}
        </select>
      </label>
      {folders.length ? (
        <label className="grid gap-2 text-xs">
          Project folder
          <select
            value={JSON.stringify([projectId ?? "", path])}
            onChange={(event) => {
              const selected = folders
                .flatMap((entry) =>
                  entry.paths.map((folder) => ({ projectId: entry.projectId, path: folder })),
                )
                .find(
                  (entry) => JSON.stringify([entry.projectId, entry.path]) === event.target.value,
                );
              if (selected) {
                setProjectId(selected.projectId);
                setPath(selected.path);
              } else {
                setProjectId(undefined);
                setPath("");
              }
            }}
            className="rounded-lg border border-border bg-default px-3 py-2 text-sm"
          >
            <option value={JSON.stringify(["", ""])}>Choose a folder or enter a path</option>
            {folders.flatMap((entry) =>
              entry.paths.map((folder) => (
                <option
                  key={JSON.stringify([entry.projectId, folder])}
                  value={JSON.stringify([entry.projectId, folder])}
                >
                  {projects.records.find((project) => project.id === entry.projectId)?.name} ·{" "}
                  {folder}
                </option>
              )),
            )}
          </select>
        </label>
      ) : null}
      <label className="grid gap-2 text-xs">
        Repository path on this node
        <input
          required
          value={path}
          onChange={(event) => {
            setPath(event.target.value);
            setProjectId(undefined);
          }}
          className="rounded-lg border border-border bg-default px-3 py-2 text-sm"
        />
      </label>
      {isDesktopNode(nodeId) && desktopDirectorySelectionAvailable() ? (
        <div>
          <Button
            type="button"
            variant="secondary"
            isDisabled={picking || node === undefined}
            onPress={() => {
              chooseDirectory().catch(pickerFailed);
            }}
          >
            {picking ? "Choosing folder…" : "Choose local folder"}
          </Button>
          <p className="mt-2 text-xs text-muted">
            Choose a folder to grant this local session access.
          </p>
        </div>
      ) : null}
      {pickerError === null ? null : (
        <p role="alert" className="text-xs text-danger">
          {pickerError}
        </p>
      )}
      <p className="text-xs text-muted">
        The connected node checks repository access. A saved project folder does not grant
        permission.
      </p>
      {availableNodes.length === 0 ? (
        <output className="text-xs text-muted">
          No connected node currently provides the complete Git capability. Enable Git and connect a
          supported execution node.
        </output>
      ) : null}
      <div>
        <Button type="submit" variant="secondary" isDisabled={!node || !path.trim()}>
          Open Git
        </Button>
      </div>
    </form>
  );
}

export function GitLauncher() {
  const { openCapabilityTab } = useClient();
  return (
    <div className="mx-auto max-w-xl p-7">
      <h2 className="mb-5 text-sm font-medium">Explore files and changes</h2>
      <GitTargetChooser
        onChoose={(target) => {
          openCapabilityTab(gitDescriptor(target));
        }}
      />
    </div>
  );
}

function CapturedGitView({ target }: { target: GitTarget }) {
  const {
    openCapabilityTab,
    createConversation,
    setConversationContext,
    projects,
    connectionState,
    gitPresentation,
  } = useClient();
  const nodes = useSource(connectionState);
  const git = useSource(gitPresentation.state);
  const { connection, error } = useGitConnection(target.nodeId, target.principal);
  const capability = git.nodes[target.nodeId];
  const available =
    git.enabled &&
    capability?.available === true &&
    capability.principal === target.principal &&
    capability.generation === connection?.generation;
  function startConversation(path: string) {
    const chat = createConversation(target.nodeId);
    setConversationContext(chat, {
      nodeId: target.nodeId,
      principal: target.principal,
      path,
      owner: "git",
    });
    if (target.projectId !== undefined && target.projectId !== "") {
      projects.stageConversation(chat.id, target.projectId).catch(projects.fail);
    }
  }
  return (
    <div className="flex h-full min-h-0 flex-col">
      {error === null ? null : (
        <p role="alert" className="p-4 text-xs text-danger">
          {error}
        </p>
      )}
      {available ? null : (
        <output className="border-b border-border px-4 py-3 text-xs text-muted">
          {git.enabled
            ? (capability?.error ??
              "Git is unavailable on this connection. The last captured view is retained.")
            : "The Git plugin is disabled. This view is retained until it is enabled again."}
        </output>
      )}
      <div className="min-h-0 flex-1">
        <GitView
          key={JSON.stringify([target.nodeId, target.principal, target.path, target.projectId])}
          connection={available ? connection : null}
          path={target.path}
          nodeLabel={nodes.nodes.find((node) => node.id === target.nodeId)?.label ?? target.nodeId}
          onStartConversation={() => {
            startConversation(target.path);
          }}
          {...(target.projectId !== undefined && target.projectId !== ""
            ? { projectId: target.projectId }
            : {})}
          onWorktreeCreated={(path) => {
            startConversation(path);
            openCapabilityTab(gitDescriptor({ ...target, path }));
          }}
        />
      </div>
    </div>
  );
}

export function GitCapabilityView({ tab }: { tab: CapabilityTab }) {
  const target = gitTarget(tab);
  return target ? (
    <CapturedGitView target={target} />
  ) : (
    <p role="alert" className="p-6 text-sm text-muted">
      This repository view has an invalid target. Open a new view to select a repository.
    </p>
  );
}

function CapturedGitSettings({ target, onChange }: { target: GitTarget; onChange: () => void }) {
  const { gitPresentation } = useClient();
  const git = useSource(gitPresentation.state);
  const { connection, error } = useGitConnection(target.nodeId, target.principal);
  const capability = git.nodes[target.nodeId];
  const available =
    git.enabled &&
    capability?.available === true &&
    capability.principal === target.principal &&
    capability.generation === connection?.generation;
  return (
    <>
      <div className="mb-6 flex items-center justify-between gap-3">
        <p className="min-w-0 truncate text-xs text-muted">
          {target.nodeId} · {target.path}
        </p>
        <button type="button" onClick={onChange} className="shrink-0 text-xs underline">
          Change repository
        </button>
      </div>
      {error === null ? null : (
        <p role="alert" className="mb-4 text-xs text-danger">
          {error}
        </p>
      )}
      {available ? null : (
        <output className="mb-4 text-xs text-muted">
          {git.enabled
            ? (capability?.error ??
              "Git is unavailable on this connection. Unsaved settings are retained.")
            : "The Git plugin is disabled. Unsaved settings are retained."}
        </output>
      )}
      <GitSettings
        key={JSON.stringify([target.nodeId, target.principal, target.path, target.projectId])}
        connection={available ? connection : null}
        path={target.path}
        {...(target.projectId !== undefined && target.projectId !== ""
          ? { projectId: target.projectId }
          : {})}
      />
    </>
  );
}

export function GitSettingsPage() {
  const [target, setTarget] = useState<GitTarget | null>(null);
  return (
    <>
      <h1 className="mb-2 text-2xl font-medium">Git</h1>
      <p className="mb-8 text-xs text-muted">
        Repository preferences and preparation on the selected execution node.
      </p>
      {target ? (
        <CapturedGitSettings
          target={target}
          onChange={() => {
            setTarget(null);
          }}
        />
      ) : (
        <GitTargetChooser onChoose={setTarget} />
      )}
    </>
  );
}
