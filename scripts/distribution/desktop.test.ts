import { describe, expect, test } from "bun:test";
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";

import {
  nativeVersionFromTag,
  releaseTarget,
  stageDesktopPackage,
  versionFromTag,
} from "./desktop";

describe("desktop release identity", () => {
  test("requires a canonical release tag before changing package identity", () => {
    expect(versionFromTag("v0.2.0")).toBe("0.2.0");
    expect(versionFromTag("v1.2.3-beta.4")).toBe("1.2.3-beta.4");
    expect(nativeVersionFromTag("v1.2.3-beta.4")).toBe("1.2.3");
    for (const tag of ["develop", "1.2.3", "v01.2.3", "v1.2.3/evil", "v1.2.3-beta.01"]) {
      expect(() => versionFromTag(tag)).toThrow();
    }
  });

  test("separates architecture feeds and refuses unsupported hosts", () => {
    expect(releaseTarget("darwin", "arm64").rid).toBe("osx-arm64");
    expect(releaseTarget("darwin", "x64").rid).toBe("osx-x64");
    expect(releaseTarget("win32", "x64").rid).toBe("win-x64");
    expect(releaseTarget("linux", "x64").rid).toBe("linux-x64");
    expect(() => releaseTarget("linux", "ia32")).toThrow();
  });
});

test("stages the executable with its loader and UI without stale files", async () => {
  const directory = await mkdtemp(path.join(os.tmpdir(), "swarmie-desktop-release-"));
  const packaged = path.join(directory, "package");
  const stage = path.join(directory, "stage");
  try {
    await mkdir(path.join(packaged, "bin"), { recursive: true });
    await mkdir(path.join(packaged, "resources/dist"), { recursive: true });
    await mkdir(stage);
    await writeFile(path.join(packaged, "bin/swarmie.exe"), "executable");
    await writeFile(path.join(packaged, "bin/WebView2Loader.dll"), "loader");
    await writeFile(path.join(packaged, "resources/dist/index.html"), "frontend");
    await writeFile(path.join(stage, "stale.js"), "old frontend");

    await stageDesktopPackage(packaged, stage);

    expect(await Bun.file(path.join(stage, "swarmie.exe")).text()).toBe("executable");
    expect(await Bun.file(path.join(stage, "WebView2Loader.dll")).text()).toBe("loader");
    expect(await Bun.file(path.join(stage, "resources/dist/index.html")).text()).toBe("frontend");
    expect(await Bun.file(path.join(stage, "stale.js")).exists()).toBe(false);
    expect(await Bun.file(path.join(stage, "bin/swarmie.exe")).exists()).toBe(false);
    expect(await Bun.file(path.join(packaged, "bin/swarmie.exe")).text()).toBe("executable");
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});
