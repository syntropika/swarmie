#!/usr/bin/env node
const { spawn } = require("node:child_process");
const path = require("node:path");
const { existsSync, readFileSync } = require("node:fs");

let target;
try {
  target = JSON.parse(readFileSync(path.join(__dirname, "../native/target.json"), "utf-8"));
} catch {
  console.error(
    "This source package needs a native executable. Build a local bundle with sdk/tools/package_local.py.",
  );
  process.exit(1);
}
if (target.platform !== process.platform || target.arch !== process.arch) {
  console.error(
    `This runtime bundle targets ${target.platform}/${target.arch}; the current host is ${process.platform}/${process.arch}.`,
  );
  process.exit(1);
}
const environment = { ...process.env };
const agentTools = path.join(__dirname, "../agent-tools");
if (!environment.SWARMIE_AGENT_TOOLS_DIR && existsSync(path.join(agentTools, "agent-tools.json"))) {
  environment.SWARMIE_AGENT_TOOLS_DIR = agentTools;
}
if (!environment.SWARMIE_NODE_EXECUTABLE) {
  environment.SWARMIE_NODE_EXECUTABLE = process.execPath;
}
const child = spawn(path.join(__dirname, "../native/swarmie"), process.argv.slice(2), {
  stdio: "inherit",
  env: environment,
});
const interrupt = () => child.kill("SIGINT");
const terminate = () => child.kill("SIGTERM");
process.on("SIGINT", interrupt);
process.on("SIGTERM", terminate);
child.on("error", () => {
  console.error("Cannot start the bundled runtime executable.");
  process.exitCode = 1;
});
child.on("exit", (code, signal) => {
  process.removeListener("SIGINT", interrupt);
  process.removeListener("SIGTERM", terminate);
  process.exitCode =
    code ??
    (() => {
      if (signal === "SIGINT") {
        return 130;
      }
      if (signal === "SIGTERM") {
        return 143;
      }
      return 1;
    })();
});
