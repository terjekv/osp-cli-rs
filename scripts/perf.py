#!/usr/bin/env python3
"""Performance budgets for the built release CLI.

Scenarios and limits live in `perf/budgets.json`. Each scenario runs the real
binary in a fresh hermetic home after one unmeasured warm-up run. Three layers
check it, from deterministic to noisy:

1. Syscalls (strace): exact counts of disk syncs, persistent writes, child
   execs and non-loopback connects. Forced syncs cost seconds on a busy disk
   while CPU time stays flat, so only syscall counts catch them reliably.
2. Instructions (cachegrind): total Ir per scenario and across scenarios
   against committed baselines partitioned by CPU and toolchain. Ir is nearly
   deterministic, unlike wall-clock time, which varied 4-21% between runs.
3. Timings (`OSP_TRACE_STARTUP=json`): min of 5 runs per phase, printed as a
   table. Only loose absolute limits fail, to catch gross stalls.

`--record` rewrites this partition's instruction baselines; committing them is
the deliberate act that accepts a slowdown. The downstream product reuses this
script through `run()` with its own budget file and fixtures.
"""

from __future__ import annotations

import argparse
import contextlib
from datetime import datetime, timezone
import fcntl
import hashlib
import ipaddress
import json
import os
from pathlib import Path
import platform
import pty
import re
import runpy
import select
import shutil
import struct
import subprocess
import sys
import tempfile
import termios
import time



SYNC_CALLS = {"fsync", "fdatasync", "sync_file_range", "sync", "syncfs"}
WRITE_CALLS = {
    "creat", "rename", "renameat", "renameat2", "link", "linkat", "symlink", "symlinkat",
    "unlink", "unlinkat", "rmdir", "mkdir", "mkdirat", "truncate",
}
OPEN_CALLS = {"open", "openat", "openat2"}
TRACED = SYNC_CALLS | WRITE_CALLS | OPEN_CALLS | {"execve", "execveat", "connect"}
SYSCALL_LINE = re.compile(r"^(\w+)\((.*)\)\s+= (-?\d+)(<[^>]*>)?")
TIMING_RUNS = 5


def binary_default(root: Path) -> Path:
    target = Path(os.environ.get("CARGO_TARGET_DIR", "target"))
    return root / target / os.environ.get("CARGO_BUILD_TARGET", "") / "release" / "osp"


@contextlib.contextmanager
def plugin_fixture(run_dir: Path):
    """A describe-capable plugin that prints a fixed 500-row dataset."""

    plugins = run_dir / "plugins"
    plugins.mkdir()
    rows = json.dumps([
        {"id": i, "name": f"host{i:03}", "team": ["core", "web", "db"][i % 3], "cpus": i % 16}
        for i in range(500)
    ])
    script = plugins / "osp-rows"
    # Shell builtins only: the scenario PATH is empty for deterministic lookups.
    script.write_text(
        "#!/bin/sh\n"
        'if [ "$1" = --osp-describe ]; then\n'
        "  printf '%s\\n' '{\"about\": \"Fixed rows\", \"output\": \"json\"}'\n"
        "else\n"
        f"  printf '%s\\n' '{rows}'\n"
        "fi\n"
    )
    script.chmod(0o755)
    yield {"OSP_PLUGIN_PATH": str(plugins)}


FIXTURES = {"none": lambda run_dir: contextlib.nullcontext({}), "plugins": plugin_fixture}


def scenario_env(run_dir: Path, extra: dict[str, str]) -> dict[str, str]:
    hermetic_env = runpy.run_path(str(Path(__file__).with_name("run-hermetic-cargo.py")))["hermetic_env"]
    home = run_dir / "home"
    temp = run_dir / "tmp"
    for path in (home, temp, run_dir / "bin"):
        path.mkdir(exist_ok=True)
    # Only the hermetic runtime keys: ambient variables such as the session bus
    # reach real services, and PATH contents vary by host while plugins scan it.
    keys = ("HOME", "LANG", "LC_ALL", "TERM", "NO_COLOR", "KRB5CCNAME")
    env = {key: value for key, value in hermetic_env(home).items() if key in keys or key.startswith("XDG_")}
    env.pop("XDG_RUNTIME_DIR", None)
    env.update({"PATH": str(run_dir / "bin"), "TMPDIR": str(temp), "USER": "perf", "LOGNAME": "perf", **extra})
    return env


def run_cli(command: list[str], env: dict[str, str], cwd: Path, keys: list[str] | None, timeout: float = 120):
    """Run one invocation; with `keys`, on a terminal that types each after the next prompt paint."""

    if keys is None:
        return subprocess.run(
            command, env=env, cwd=cwd, stdin=subprocess.DEVNULL,
            capture_output=True, text=True, timeout=timeout,
        )
    master, slave = pty.openpty()
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 24, 80, 0, 0))

    def controlling_terminal():
        os.setsid()
        fcntl.ioctl(0, termios.TIOCSCTTY, 0)

    child = subprocess.Popen(
        command, env=env, cwd=cwd, stdin=slave, stdout=slave, stderr=slave,
        preexec_fn=controlling_terminal,
    )
    os.close(slave)
    transcript = bytearray()
    deadline = time.monotonic() + timeout
    keys = list(keys)
    raw_from = None
    last_output = time.monotonic()
    try:
        while True:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                child.kill()
                raise TimeoutError(f"REPL did not exit; output:\n{transcript.decode(errors='replace')}")
            if select.select([master], [], [], min(remaining, 0.05))[0]:
                try:
                    chunk = os.read(master, 65536)
                except OSError:
                    break
                if not chunk:
                    break
                transcript.extend(chunk)
                last_output = time.monotonic()
                for _ in range(chunk.count(b"\x1b[6n")):
                    os.write(master, b"\x1b[1;1R")
            # The editor's cursor-position queries read input and swallow keys
            # typed meanwhile; it waits for input once a raw-mode paint (cursor
            # shown) is followed by silence. Only sequencing depends on this.
            if raw_from is None and not termios.tcgetattr(master)[3] & termios.ECHO:
                raw_from = len(transcript)
            if (
                keys and raw_from is not None and b"\x1b[?25h" in transcript[raw_from:]
                and time.monotonic() - last_output > 0.3
            ):
                os.write(master, keys.pop(0).encode())
                raw_from = len(transcript)
    finally:
        os.close(master)
        returncode = child.wait(timeout=max(deadline - time.monotonic(), 1))
    output = transcript.decode(errors="replace")
    return subprocess.CompletedProcess(command, returncode, output, output)


class Scenario:
    def __init__(self, name: str, spec: dict, defaults: dict, binary: Path, fixtures):
        self.name = name
        self.spec = {**defaults, **spec}
        self.binary = binary
        self.fixture = fixtures[self.spec["fixture"]]

    @contextlib.contextmanager
    def prepared(self):
        """A fresh hermetic home, warmed by one unmeasured run unless `warm = false`."""

        with tempfile.TemporaryDirectory(prefix=f"osp-perf-{self.name}-") as directory:
            run_dir = Path(directory).resolve()
            with self.fixture(run_dir) as extra:
                env = scenario_env(run_dir, extra)
                if self.spec.get("warm", True):
                    self.invoke([], env, run_dir)
                yield env, run_dir

    def invoke(self, prefix: list[str], env: dict[str, str], run_dir: Path):
        completed = run_cli(
            [*prefix, str(self.binary), *self.spec.get("argv", [])],
            env, run_dir / "tmp", self.spec.get("repl_keys"),
        )
        if completed.returncode:
            raise SystemExit(
                f"perf: scenario `{self.name}` exited {completed.returncode}\n"
                f"stdout:\n{completed.stdout[-2000:]}\nstderr:\n{completed.stderr[-2000:]}"
            )
        return completed


def remote_address(arguments: str) -> str | None:
    match = re.search(r'inet_addr\("([^"]+)"\)|inet_pton\(AF_INET6, "([^"]+)"', arguments)
    if not match:
        return None
    address = ipaddress.ip_address(match[1] or match[2])
    if getattr(address, "ipv4_mapped", None):
        address = address.ipv4_mapped
    return None if address.is_loopback else str(address)


def write_paths(call: str, arguments: str, result_path: str | None) -> list[str]:
    if call in OPEN_CALLS:
        flags = arguments.split(", ")[1 if call == "open" else 2]
        if not re.search(r"O_WRONLY|O_RDWR|O_CREAT|O_TRUNC", flags):
            return []
        return [result_path[1:-1]] if result_path else []
    paths = []
    for directory, path in re.findall(r'(?:(?:AT_FDCWD|\d+)<([^>]*)>, )?"([^"]*)"', arguments):
        paths.append(path if path.startswith("/") or not directory else f"{directory}/{path}")
    return paths


def trace_syscalls(scenario: Scenario, strace: str) -> dict[str, list[str]]:
    """Count budgeted syscalls across the process tree; returns evidence per budget key."""

    with scenario.prepared() as (env, run_dir):
        trace_dir = Path(tempfile.mkdtemp(prefix="osp-perf-strace-"))
        try:
            scenario.invoke(
                [strace, "-ff", "-qq", "-y", "-s", "4096", "-e", "trace=" + ",".join(sorted(TRACED)),
                 "-o", str(trace_dir / "trace")],
                env, run_dir,
            )
            lines = [line for path in sorted(trace_dir.iterdir()) for line in path.read_text().splitlines()]
        finally:
            shutil.rmtree(trace_dir)
    temp = str(run_dir / "tmp") + "/"
    evidence: dict[str, list[str]] = {"syncs": [], "persistent_writes": [], "execs": [], "remote_connects": []}
    for line in lines:
        match = SYSCALL_LINE.match(line)
        if not match:
            continue
        call, arguments, result, result_path = match.groups()
        if call in SYNC_CALLS:
            evidence["syncs"].append(line)
        elif call == "connect" and remote_address(arguments):
            evidence["remote_connects"].append(line)
        elif result.startswith("-"):
            continue
        elif call in {"execve", "execveat"}:
            evidence["execs"].append(line)
        elif call in WRITE_CALLS or call in OPEN_CALLS:
            if any(not (path + "/").startswith((temp, "/dev/", "/proc/")) for path in write_paths(call, arguments, result_path)):
                evidence["persistent_writes"].append(line)
    own_exec = next(line for line in evidence["execs"] if line.startswith(f'execve("{scenario.binary}"'))
    evidence["execs"].remove(own_exec)
    return evidence


def check_syscalls(scenarios: list[Scenario]) -> list[str]:
    strace = shutil.which("strace")
    if not strace or not sys.platform.startswith("linux"):
        print("perf: SKIPPED syscall budgets: strace on Linux is required.")
        return ["syscalls"]
    failures = []
    print("\nSyscall budgets (exact):")
    for scenario in scenarios:
        evidence = trace_syscalls(scenario, strace)
        counts = {key: len(lines) for key, lines in evidence.items()}
        print(f"  {scenario.name:<20} " + "  ".join(f"{key}={count}" for key, count in counts.items()))
        for key, lines in evidence.items():
            budget = scenario.spec[key]
            if len(lines) != budget:
                relation = "exceeds" if len(lines) > budget else "is below"
                failures.append(
                    f"scenario `{scenario.name}`: {key} count {len(lines)} {relation} budget {budget}"
                    + "".join(f"\n    {line[:300]}" for line in lines[:8])
                )
    return failures


def partition(root: Path) -> str:
    cpu = next(
        (line.split(":", 1)[1] for line in Path("/proc/cpuinfo").read_text().splitlines() if line.startswith("model name")),
        platform.processor(),
    )
    rustc = subprocess.check_output(["rustc", "--version"], cwd=root, text=True).split()[1]
    return re.sub(r"[^a-z0-9.]+", "-", f"{platform.machine()} {cpu} rustc {rustc}".lower()).strip("-")


def count_instructions(scenario: Scenario, valgrind: str) -> int:
    with scenario.prepared() as (env, run_dir):
        output = run_dir / "cachegrind.out"
        scenario.invoke(
            [valgrind, "-q", "--tool=cachegrind", "--cache-sim=no", f"--cachegrind-out-file={output}"],
            env, run_dir,
        )
        return int(re.search(r"^summary: (\d+)", output.read_text(), re.M)[1])


def check_instructions(root: Path, scenarios: list[Scenario], gates: dict, record: bool) -> list[str]:
    valgrind = shutil.which("valgrind")
    if not valgrind:
        print("perf: SKIPPED instruction budgets: valgrind is required.")
        return ["instructions"]
    scenarios = [scenario for scenario in scenarios if scenario.spec.get("measure")]
    key = partition(root)
    directory = root / "perf" / "baselines" / key
    current = {scenario.name: count_instructions(scenario, valgrind) for scenario in scenarios}
    if record:
        shutil.rmtree(directory, ignore_errors=True)
        directory.mkdir(parents=True)
        identity = {
            "partition": key,
            "host": platform.node(),
            "platform": platform.platform(),
            "rustc": subprocess.check_output(["rustc", "--version"], cwd=root, text=True).strip(),
            "valgrind": subprocess.check_output([valgrind, "--version"], text=True).strip(),
            "source_revision": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip(),
            "source_dirty": bool(subprocess.check_output(["git", "status", "--porcelain"], cwd=root)),
            "binary_sha256": hashlib.sha256(scenarios[0].binary.read_bytes()).hexdigest(),
            "recorded_at": datetime.now(timezone.utc).isoformat(timespec="seconds"),
        }
        for name, instructions in current.items():
            (directory / f"{name}.json").write_text(json.dumps({"instructions": instructions, **identity}, indent=2) + "\n")
        print(f"\nRecorded {len(current)} instruction baselines in {directory.relative_to(root)}; review before committing.")
        return []
    if not directory.is_dir():
        return [
            f"no instruction baseline for partition `{key}`; record one with "
            f"`python3 scripts/perf.py --record` and commit perf/baselines/{key}"
        ]
    failures = []
    limit = gates["instructions_percent"]
    print(f"\nInstruction budgets (partition {key}; +{limit}% per scenario):")
    totals = [0, 0]
    for name, instructions in current.items():
        path = directory / f"{name}.json"
        if not path.is_file():
            failures.append(f"scenario `{name}` has no instruction baseline in {path.relative_to(root)}")
            continue
        baseline = json.loads(path.read_text())["instructions"]
        change = (instructions / baseline - 1) * 100
        totals[0] += instructions
        totals[1] += baseline
        print(f"  {name:<20} {instructions:>14,} Ir  baseline {baseline:>14,}  {change:+6.2f}%")
        if change > limit:
            failures.append(f"scenario `{name}`: {instructions:,} Ir is {change:+.2f}% over baseline {baseline:,} (budget +{limit}%)")
    if totals[1]:
        change = (totals[0] / totals[1] - 1) * 100
        limit = gates["instructions_total_percent"]
        print(f"  {'total':<20} {totals[0]:>14,} Ir  baseline {totals[1]:>14,}  {change:+6.2f}%  (budget +{limit}%)")
        if change > limit:
            failures.append(f"total: {totals[0]:,} Ir is {change:+.2f}% over baseline {totals[1]:,} (budget +{limit}%)")
    return failures


def check_timings(scenarios: list[Scenario], gates: dict) -> list[str]:
    failures = []
    print(f"\nStartup timings (min of {TIMING_RUNS}; fail only above {gates['phase_ms']} ms per phase or {gates['total_ms']} ms total):")
    for scenario in (scenario for scenario in scenarios if scenario.spec.get("measure")):
        phases: dict[str, float] = {}
        total = float("inf")
        with scenario.prepared() as (env, run_dir):
            env["OSP_TRACE_STARTUP"] = "json"
            for _ in range(TIMING_RUNS):
                stderr = scenario.invoke([], env, run_dir).stderr
                trace = json.JSONDecoder().raw_decode(stderr, stderr.rindex('{"total_ms"'))[0]
                total = min(total, trace["total_ms"])
                for phase in trace["phases"]:
                    name = "  " * phase["depth"] + phase["name"]
                    phases[name] = min(phases.get(name, float("inf")), phase["duration_ms"])
        print(f"  {scenario.name:<32} {total:8.1f} ms")
        for name, duration in phases.items():
            if not name.startswith("    "):
                print(f"    {name[:30]:<30} {duration:8.1f} ms")
            if duration > gates["phase_ms"]:
                failures.append(f"scenario `{scenario.name}`: phase `{name.strip()}` took {duration:.1f} ms (limit {gates['phase_ms']} ms)")
        if total > gates["total_ms"]:
            failures.append(f"scenario `{scenario.name}`: total {total:.1f} ms (limit {gates['total_ms']} ms)")
    return failures


def run(root: Path, fixtures: dict | None = None) -> None:
    """Check `root`'s perf/budgets.json; `fixtures` adds product fixture factories."""

    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--binary", type=Path, default=binary_default(root))
    parser.add_argument("--record", action="store_true", help="Rewrite this partition's instruction baselines.")
    parser.add_argument("--layer", choices=["syscalls", "instructions", "timings"], action="append")
    parser.add_argument("scenario", nargs="*", help="Limit the run to these scenarios.")
    args = parser.parse_args()
    if args.record and (args.scenario or args.layer):
        parser.error("--record re-records every measured scenario")
    binary = args.binary.resolve()
    if not binary.is_file():
        parser.error(f"release binary missing: {binary}; run confidence.py --check build first")
    budgets = json.loads((root / "perf" / "budgets.json").read_text())
    scenarios = [
        Scenario(name, spec, budgets["defaults"], binary, {**FIXTURES, **(fixtures or {})})
        for name, spec in budgets["scenarios"].items()
        if not args.scenario or name in args.scenario
    ]
    started = time.perf_counter()
    if args.record:
        check_instructions(root, scenarios, budgets["gates"], record=True)
        return
    layers = args.layer or ["syscalls", "instructions", "timings"]
    failures, skipped = [], []
    if "syscalls" in layers:
        result = check_syscalls(scenarios)
        (skipped if result == ["syscalls"] else failures).extend(result)
    if "instructions" in layers:
        result = check_instructions(root, scenarios, budgets["gates"], record=False)
        (skipped if result == ["instructions"] else failures).extend(result)
    if "timings" in layers:
        failures.extend(check_timings(scenarios, budgets["gates"]))
    elapsed = time.perf_counter() - started
    if failures:
        sys.stdout.flush()
        print(f"\nperf: FAILED after {elapsed:.0f}s", file=sys.stderr)
        for failure in failures:
            print(f"  - {failure}", file=sys.stderr)
        raise SystemExit(1)
    print(f"\nperf: OK in {elapsed:.0f}s" + (f"; SKIPPED layers: {', '.join(skipped)}" if skipped else ""))


if __name__ == "__main__":
    run(Path(__file__).resolve().parent.parent)
