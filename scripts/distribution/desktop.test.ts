import { describe, expect, test } from "bun:test";

import { releaseTarget, versionFromTag } from "./desktop";

describe("desktop release identity", () => {
  test("requires a canonical release tag before changing package identity", () => {
    expect(versionFromTag("v0.2.0")).toBe("0.2.0");
    expect(versionFromTag("v1.2.3-beta.4")).toBe("1.2.3-beta.4");
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
