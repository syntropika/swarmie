"""Run inside GDB's Python interpreter, only through the diagnostic wrapper.

All GDB API operations stay on GDB's thread. The timer only signals the inferior.
Marker stops and stack collection perturb the workload; raw timings are diagnostic.
"""

import json
import os
from pathlib import Path
import signal
import threading
import time

import gdb


def main():
    config = json.loads(Path(os.environ["SWARMIE_DIAGNOSTIC_GDB_CONFIG"]).read_text())
    output = Path(config["output"])
    state = {"phase": "setup", "run_index": 0, "thread": None, "exit_code": None}
    samples = []
    windows = []
    pauses = []
    begin = gdb.Breakpoint("swarmie_diagnostic_execution_begin", internal=True)
    end = gdb.Breakpoint("swarmie_diagnostic_execution_end", internal=True)

    def record_inferior():
        pid = gdb.selected_inferior().pid
        fields = Path(f"/proc/{pid}/stat").read_text().rsplit(") ", 1)[1].split()
        (output / "inferior.json").write_text(json.dumps({
            "pid": pid, "start_ticks": fields[19]}))

    def stopped(event):
        state.update(stopped_ns=time.monotonic_ns(), stop_kind="other")
        if isinstance(event, gdb.BreakpointEvent):
            if begin in event.breakpoints:
                state.update(phase="execution", run_index=state["run_index"] + 1,
                             thread=gdb.selected_thread(), stop_kind="begin-marker")
                windows.append({"run_index": state["run_index"], "begin_ns": time.monotonic_ns(),
                                "ptid": list(state["thread"].ptid), "samples": 0})
                record_inferior()
            elif end in event.breakpoints:
                state["phase"] = "validation"
                state["stop_kind"] = "end-marker"
                windows[-1]["end_ns"] = time.monotonic_ns()
        elif isinstance(event, gdb.SignalEvent) and event.stop_signal == "SIGINT":
            state["stop_kind"] = "sample"

    def resume(capture=False):
        now = time.monotonic_ns()
        if "stopped_ns" in state:
            pauses.append({"run_index": state["run_index"], "kind": state["stop_kind"],
                           "stop_notification_to_resume_ns": now - state.pop("stopped_ns")})
        return gdb.execute("continue", to_string=capture)

    def exited(event):
        state["exit_code"] = getattr(event, "exit_code", None)
        state["phase"] = "exited"

    gdb.events.stop.connect(stopped)
    gdb.events.exited.connect(exited)
    gdb.execute("set pagination off")
    gdb.execute("set confirm off")
    gdb.execute("set debuginfod enabled off")
    gdb.execute("set startup-with-shell off")
    gdb.execute("handle SIGINT stop nopass")
    gdb.execute("starti")
    record_inferior()
    resume()
    while state["phase"] != "exited":
        if state["phase"] != "execution" or windows[-1]["samples"] >= config["max_samples"]:
            resume()
            continue
        pid = gdb.selected_inferior().pid
        timer = threading.Timer(config["interval_ms"] / 1000, os.kill, (pid, signal.SIGINT))
        started = time.monotonic_ns()
        timer.start()
        try:
            resume(capture=True)
        finally:
            timer.cancel()
            timer.join()
        if state["phase"] != "execution":
            continue
        stopped_ns = time.monotonic_ns()
        runtime = state["thread"]
        if not runtime.is_valid():
            raise RuntimeError("runtime thread exited before the end marker")
        runtime.switch()
        frames = []
        error = None
        frame = gdb.newest_frame()
        try:
            while frame is not None and len(frames) < config["max_frames"]:
                frames.append({"name": frame.name(), "pc": hex(frame.pc())})
                frame = frame.older()
        except gdb.error as exception:
            error = str(exception)
        sample = {"run_index": state["run_index"], "ptid": list(runtime.ptid),
                  "stop_ns": stopped_ns, "execution_elapsed_ns": stopped_ns - windows[-1]["begin_ns"],
                  "resume_to_stop_ns": stopped_ns - started,
                  "collection_ns": time.monotonic_ns() - stopped_ns,
                  "frames": frames, "truncated": frame is not None, "unwind_error": error}
        samples.append(sample)
        windows[-1]["samples"] += 1
        with (output / "stacks.ndjson").open("a") as stream:
            stream.write(json.dumps(sample) + "\n")
    Path(output / "sampler.json").write_text(json.dumps({
        "schema_version": 1, "exit_code": state["exit_code"], "windows": windows,
        "sample_count": len(samples),
        "max_samples_reached": any(w["samples"] >= config["max_samples"] for w in windows),
        "sample_budget": "per repetition; reaching the cap leaves later execution unsampled",
        "interval_ms": config["interval_ms"], "max_frames": config["max_frames"],
        "collection_ns": sum(s["collection_ns"] for s in samples),
        "debugger_pauses": pauses,
        "sampling_stop_notification_to_resume_ns": sum(
            p["stop_notification_to_resume_ns"] for p in pauses if p["kind"] == "sample"),
        "pause_limit": "Stop-notification to resume-command estimate; excludes interrupt delivery and GDB resume processing. Marker pauses are outside workload snapshots.",
        "scope": "runtime thread selected at begin marker; all inferior threads stop during collection",
    }, indent=2) + "\n")
    if state["exit_code"] != 0 or not samples or any("end_ns" not in w for w in windows):
        raise RuntimeError("incomplete profile: missing samples, boundary, or successful inferior exit")


try:
    main()
except Exception as error:
    print("DIAGNOSTIC_SAMPLER_ERROR:", error)
    gdb.execute("quit 1")
