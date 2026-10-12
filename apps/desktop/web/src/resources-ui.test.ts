import { describe, expect, it } from "vitest";
import {
  matchingTemplateIdentifier,
  parseTemplateAliases,
  templateParameterNames,
} from "./ResourceLibrary";
import {
  parseNativeDefaults,
  projectDefaultsPayload,
  projectWorkspacePaths,
} from "./ProjectSettings";
import type { Project } from "./protocol";

describe("template command parameters and aliases", () => {
  it("uses unique canonical placeholder keys including dots and hyphens", () => {
    expect(
      templateParameterNames(
        "Review {{ file }} and {{file}} for {{review.mode}}: {{test-command}}",
      ),
    ).toEqual(["file", "review.mode", "test-command"]);
  });
  it("resolves a matched alias as the render identifier rather than the display name", () => {
    const template = {
      id: "internal-uuid",
      name: "Code review",
      text: "Review {{file}}",
      aliases: ["review", "inspect"],
    };
    expect(matchingTemplateIdentifier(template, "inspect")).toBe("inspect");
    expect(matchingTemplateIdentifier(template, "INSPECT")).toBe("inspect");
    expect(matchingTemplateIdentifier(template, "code")).toBe("internal-uuid");
    expect(matchingTemplateIdentifier(template, "missing")).toBeUndefined();
  });
  it("accepts slash-prefixed labels but rejects duplicate and invalid aliases", () => {
    expect(parseTemplateAliases("/review, inspect test-command")).toEqual([
      "review",
      "inspect",
      "test-command",
    ]);
    expect(parseTemplateAliases("")).toEqual([]);
    for (const aliases of [
      "review review",
      "review/invalid",
      Array.from({ length: 17 }, (_, i) => `a${i}`).join(" "),
    ])
      expect(() => parseTemplateAliases(aliases)).toThrow();
  });
});

describe("project default controls and workspace selection", () => {
  const project: Project = {
    id: "project",
    revision: 3,
    path: "/repo",
    name: "Repo",
    worktrees: [
      {
        id: "main",
        path: "/repo",
        branch: "main",
        isMain: true,
        locked: false,
        prunable: false,
      },
      {
        id: "feature",
        path: "/trees/feature",
        branch: "feature",
        isMain: false,
        locked: false,
        prunable: false,
      },
      {
        id: "missing",
        path: "/trees/missing",
        branch: null,
        isMain: false,
        locked: false,
        prunable: true,
      },
    ],
  };
  it("keeps project identity while selecting an existing worktree", () => {
    expect(projectWorkspacePaths(project)).toEqual(["/repo", "/trees/feature"]);
    expect(
      projectDefaultsPayload(
        project,
        "small",
        '{"permissionMode":"plan","maxTokens":512}',
        "/trees/feature",
        "claude",
      ),
    ).toEqual({
      projectId: "project",
      expectedRevision: 3,
      model: "small",
      config: { permissionMode: "plan", maxTokens: 512 },
      directoryPath: "/trees/feature",
      harness: "claude",
    });
  });
  it("clears defaults explicitly and refuses missing or unrelated workspace paths", () => {
    expect(projectDefaultsPayload(project, "", "null", "")).toEqual({
      projectId: "project",
      expectedRevision: 3,
      model: null,
      config: null,
      directoryPath: "/repo",
    });
    expect(() =>
      projectDefaultsPayload(project, "", "{}", "/trees/missing"),
    ).toThrow();
    expect(() =>
      projectDefaultsPayload(project, "", "{}", "/other-project"),
    ).toThrow();
    expect(parseNativeDefaults("null")).toEqual({});
    for (const config of ["[]", '"plan"', "broken"])
      expect(() => parseNativeDefaults(config)).toThrow();
  });
});
