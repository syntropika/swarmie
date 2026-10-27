import { cp, mkdir, readFile, readdir, rm, writeFile } from "node:fs/promises";
import path from "node:path";

const root = path.resolve(import.meta.dirname, "../..");
const desktop = path.join(root, "apps/desktop-client");

export function versionFromTag(tag: string): string {
  const version = tag.startsWith("v") ? tag.slice(1) : "";
  const numeric = "(?:0|[1-9][0-9]*)";
  const prerelease =
    "(?:-(?:0|[1-9][0-9]*|[0-9]*[a-zA-Z-][0-9a-zA-Z-]*)(?:\\.(?:0|[1-9][0-9]*|[0-9]*[a-zA-Z-][0-9a-zA-Z-]*))*)?";
  if (!new RegExp(`^${numeric}\\.${numeric}\\.${numeric}${prerelease}$`, "u").test(version)) {
    throw new Error("Desktop release tags must be vMAJOR.MINOR.PATCH with an optional prerelease.");
  }
  return version;
}

export function nativeVersionFromTag(tag: string): string {
  return versionFromTag(tag).replace(/-.*/u, "");
}

export function releaseTarget(platform = process.platform, arch = process.arch) {
  const target = new Map([
    ["darwin", { os: "osx", native: "macos" }],
    ["linux", { os: "linux", native: "linux" }],
    ["win32", { os: "win", native: "windows" }],
  ]).get(platform);
  if (target === undefined || (arch !== "x64" && arch !== "arm64")) {
    throw new Error(`Unsupported desktop release target: ${platform}/${arch}`);
  }
  return {
    ...target,
    rid: `${target.os}-${arch}`,
  };
}

export async function stageDesktopPackage(packaged: string, stage: string) {
  await rm(stage, { recursive: true, force: true });
  await mkdir(stage, { recursive: true });
  await cp(packaged, stage, { recursive: true });
  await cp(path.join(packaged, "bin"), stage, { recursive: true });
  await rm(path.join(stage, "bin"), { recursive: true, force: true });
}

async function run(arguments_: string[], cwd = root) {
  const child = Bun.spawn(arguments_, {
    cwd,
    stdout: "inherit",
    stderr: "inherit",
    env: process.env,
  });
  if ((await child.exited) !== 0) {
    throw new Error(`Desktop command failed: ${arguments_[0]}`);
  }
}

async function native(arguments_: string[]) {
  await run(
    ["node", path.join(root, "node_modules/@native-sdk/cli/bin/native.js"), ...arguments_],
    desktop,
  );
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

async function readManifest() {
  const value: unknown = JSON.parse(await readFile(path.join(desktop, "app.json"), "utf-8"));
  if (!isRecord(value)) {
    throw new Error("The desktop manifest must be an object.");
  }
  return value;
}

async function prepare(tag: string) {
  const manifest = await readManifest();
  manifest.version = nativeVersionFromTag(tag);
  await writeFile(path.join(desktop, "app.json"), `${JSON.stringify(manifest, null, 2)}\n`);
}

async function build() {
  await run(["bun", "run", "build:desktop:updater"]);
  await native(["build", ".", "--yes", ...targetFlags()]);
}

function targetFlags() {
  return process.platform === "win32" ? ["-Dtarget=x86_64-windows-msvc"] : [];
}

async function activation() {
  const executable = path.join(
    desktop,
    "zig-out/bin",
    process.platform === "win32" ? "swarmie.exe" : "swarmie",
  );
  const hooks = [
    "--veloapp-install",
    "--veloapp-updated",
    "--veloapp-obsolete",
    "--veloapp-uninstall",
  ];
  await Promise.all(
    hooks.map(async (hook) => {
      const child = Bun.spawn([executable, hook, "0.1.0"], {
        stdout: "pipe",
        stderr: "pipe",
        timeout: 5000,
      });
      const status = await child.exited;
      if (status !== 0) {
        throw new Error(`Desktop activation hook ${hook} failed (${status}).`);
      }
    }),
  );
}

async function packageRelease(tag: string) {
  const version = versionFromTag(tag);
  const manifest = await readManifest();
  if (manifest.version !== nativeVersionFromTag(tag)) {
    throw new Error("Prepare and build the release tag before packaging it.");
  }
  const target = releaseTarget();
  const output = path.join(root, "target/desktop-release", target.rid);
  const stage = path.join(root, "target/desktop-stage", target.rid);
  await mkdir(output, { recursive: true });
  await native(["package", "--target", target.native, "--signing", "adhoc"]);
  const packaged = path.join(
    desktop,
    "zig-out/package",
    target.os === "osx" ? "swarmie.app" : `swarmie-${target.native}`,
  );
  let packDirectory = packaged;
  let icon = path.join(desktop, "assets/icon.png");
  if (target.os === "osx") {
    icon = path.join(packaged, "Contents/Resources/AppIcon.icns");
  } else {
    await stageDesktopPackage(packaged, stage);
    packDirectory = stage;
    if (target.os === "win") {
      const files = await readdir(stage, { recursive: true });
      const generated = files.find((file) => file.endsWith(".ico"));
      if (generated === undefined) {
        throw new Error("Native packaging did not generate a Windows icon.");
      }
      icon = path.join(stage, generated);
    }
  }
  const tool =
    process.env.VPK_PATH ??
    path.join(root, "target/desktop-tools", process.platform === "win32" ? "vpk.exe" : "vpk");
  const arguments_ = [
    tool,
    "--yes",
    "--skip-updates",
    "pack",
    "--packId",
    "app.swarmie.desktop",
    "--packVersion",
    version,
    "--packTitle",
    "Swarmie",
    "--packAuthors",
    "Syntropika",
    "--packDir",
    packDirectory,
    "--mainExe",
    target.os === "win" ? "swarmie.exe" : "swarmie",
    "--runtime",
    target.rid,
    "--channel",
    target.rid,
    "--icon",
    icon,
    "--outputDir",
    output,
  ];
  const signingIdentity = process.env.DESKTOP_MACOS_SIGNING_IDENTITY;
  if (target.os === "osx" && signingIdentity !== undefined && signingIdentity.length > 0) {
    arguments_.push("--signAppIdentity", signingIdentity);
  }
  await run(arguments_);
  console.log(`Desktop release artifacts: ${path.relative(root, output)}`);
}

async function main() {
  const [command, tagArgument] = process.argv.slice(2);
  const tag = tagArgument ?? process.env.GITHUB_REF_NAME ?? "";
  if (command === "prepare") {
    await prepare(tag);
  } else if (command === "build") {
    await build();
  } else if (command === "package") {
    await packageRelease(tag);
  } else if (command === "activation") {
    await activation();
  } else if (command === "test") {
    await native(["test", ".", "--yes", "-Dplatform=null", ...targetFlags()]);
  } else if (command === "dev") {
    await native(["dev", ".", "--yes", ...targetFlags(), ...process.argv.slice(3)]);
  } else {
    throw new Error("Use desktop.ts prepare <tag>, build, or package <tag>.");
  }
}

if (import.meta.main) {
  await main();
}
