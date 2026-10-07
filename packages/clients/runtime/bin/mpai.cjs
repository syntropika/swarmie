#!/usr/bin/env node
const { spawn } = require("node:child_process");
const { join } = require("node:path");
let target;
try { target = require("../native/target.json"); }
catch { console.error("This source package needs a native executable. Build a local bundle with packages/clients/tools/package_local.py."); process.exit(1); }
if (target.platform !== process.platform || target.arch !== process.arch) {
  console.error(`This runtime bundle targets ${target.platform}/${target.arch}; the current host is ${process.platform}/${process.arch}.`);
  process.exit(1);
}
const child = spawn(join(__dirname, "../native/mpai"), process.argv.slice(2), { stdio: "inherit" });
const interrupt = () => child.kill("SIGINT");
const terminate = () => child.kill("SIGTERM");
process.on("SIGINT", interrupt); process.on("SIGTERM", terminate);
child.on("error", () => { console.error("Cannot start the bundled runtime executable."); process.exitCode = 1; });
child.on("exit", (code, signal) => { process.removeListener("SIGINT", interrupt); process.removeListener("SIGTERM", terminate); process.exitCode = code ?? (signal === "SIGINT" ? 130 : signal === "SIGTERM" ? 143 : 1); });
