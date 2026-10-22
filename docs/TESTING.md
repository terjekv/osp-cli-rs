# Testing and verification

Choose the smallest existing check that exercises the changed behavior.
Run broader checks when the change affects several components or a focused
check leaves an interaction unverified.

Do not add per-bug regression tests. Strengthen the existing boundary scenario
when a bug reveals a missing promise. Deleting duplicate, internal-detail or
obsolete tests is routine and needs no separate authorization; name the retained
owner in the commit body. Remove tests that pin removed features or absent
options. Verify with existing integration, contract, end-to-end and architecture
checks, static checks, and focused manual execution. Adjust existing tests when
the public contract changes.

## Choose a test target

| Existing target | Covers |
| --- | --- |
| `contracts` | Spawned CLI behavior, help, output, exit codes and configuration |
| `integration` | In-process flows across configuration, dispatch, plugins, guides and DSL evaluation |
| `e2e` | Real process/PTY behavior, prompts, completion and multi-command REPL state |
| `architecture` | Core, UI and DSL module dependency boundaries |
| `toolchain` | Stable/Miri pins in metadata and selected workflow jobs |
| `unit` and `src/**/tests*` | Local invariants, parser edges and awkward failure paths |

Prefer contracts or integration for stable behavior. Use existing PTY checks
when terminal behavior matters; a string-render check cannot prove prompt
redraw or live progress. Existing unit tests remain useful when they explain
an invariant the outer contract does not cover.

Read the target fixtures before running a suite. Contract and integration checks
isolate HOME/XDG roots, ambient `OSP_*` values and the Kerberos credential cache.
An active host ticket must not change a password fixture's authentication flow.
Fixtures that exercise Kerberos supply their own commands and credentials.
Live service requests, authentication and writes need a separately verified live
run; local fixtures do not establish those effects.

When checking saved configuration or machine-readable output, parse it and
assert meaningful values; counts or field presence alone can pass even when
the content is wrong.

Linux contracts include one real native-keyring lifecycle. It requires
`dbus-run-session`, `dbus-daemon`, `gnome-keyring-daemon` and `gdbus`; on
Debian/Ubuntu these come from `dbus-daemon`, `gnome-keyring` and
`libglib2.0-bin`. CI installs them explicitly. The confidence runner checks for
these tools before compiling tests, contracts or coverage, and fails if one is
missing. Build and publication checks do not require these services.
The runner never installs system packages implicitly.

The contract starts a private D-Bus session and unlocks a throwaway GNOME
keyring under isolated HOME/XDG roots. It executes the real CLI to persist,
reload, redact, reveal, preview, rotate and remove a scoped credential, then
verifies the ordinary-config fallback and the value-free `0600` index. The
same session makes the readable index parent temporarily unwritable, confirms
that interrupted replacement/removal restore the native credential, then
restores write access and completes rotation/removal. Run this contract as an
unprivileged user so native filesystem permission checks apply. The fixture
restores the original parent permissions even when a subprocess fails.
It preserves `LLVM_PROFILE_FILE` for instrumented child processes. This proves
the Linux Secret Service boundary; it does not verify macOS Keychain, Windows
Credential Manager or availability of a user's desktop keyring. A denied
local socket bind is an environment failure, and missing tools do not skip the
contract.

## Run existing checks

The confidence runner defines command lists, environment isolation and which
checks each lane includes. See [CONTRIBUTING.md](../CONTRIBUTING.md#verification)
for tooling, hooks, CI, coverage and release procedures.

```bash
python3 scripts/confidence.py --list
python3 scripts/confidence.py local
```

For a focused existing target:

```bash
python3 scripts/confidence.py --check architecture
python3 scripts/confidence.py --check toolchain
python3 scripts/confidence.py --check contracts
python3 scripts/confidence.py --check integration
python3 scripts/confidence.py --check e2e
```

Run the selected check once while code and dependencies remain unchanged.
Broaden validation when a failure, dependency change or unresolved interaction
requires it. Confirm tests actually executed and distinguish environment/tooling
failures from behavior failures. Report commands, results and unverified behavior;
test counts alone do not establish readiness.

Coverage checks execute cases serially, including the generic check used by the
product. Concurrent instrumented CLI and PTY starts have exceeded the fixtures'
request deadlines on this machine; ordinary behavior checks remain parallel.
Cargo still compiles dependencies in parallel.

Coverage alone uses the pinned Miri nightly and excludes only `#[cfg(test)]`
modules/helpers with `#[cfg_attr(coverage_nightly, coverage(off))]`; the gate
rejects any exclusion without an immediately preceding `#[cfg(test)]`.
Normal builds, lint and tests remain on stable Rust.

Publication rehearsal uses `--check publish-dry-run` and its own
`target/publish-dry-run` directory. Sharing the normal build directory can
leave Cargo fingerprints pointing at `target/package/...` source, allowing a
process test to execute an older packaged binary. If a current source edit
does not rebuild the CLI, inspect the package paths in `target/debug/.fingerprint`.
Run `cargo clean --package osp-cli`, then rebuild and rerun the affected check.
This removes the crate's artifacts while retaining dependency builds.

## Performance budgets

The `perf` lane builds the release binary and runs `scripts/perf.py`. It is
not part of `local`, `pre-push` or `full`; run it when a change can affect
startup, persistence or a hot path:

```bash
python3 scripts/confidence.py perf
python3 scripts/perf.py --layer syscalls help   # one layer or scenario
```

`perf/budgets.json` declares the scenarios and their limits. Each scenario
runs once unmeasured in a fresh hermetic home, then:

- under `strace` it must match exact counts of disk syncs, persistent writes
  outside its temporary directory, child execs and non-loopback connects.
  Read-only scenarios allow none; commands that persist declare their counts.
  A failure names the scenario, budget and offending syscalls.
- scenarios with `measure = true` run under cachegrind. Total instructions may
  rise 5% per scenario and 3% in total over the committed baseline for this
  CPU and toolchain in `perf/baselines/<partition>/`. Repeated runs differ by
  about 0.01%.
- the same scenarios report `OSP_TRACE_STARTUP=json` phases as the minimum of
  five runs. Only gross stalls fail: a phase over 200 ms or a total over 500 ms.

The lane needs Linux `strace` and `valgrind`; a missing tool skips its layer
with a printed `SKIPPED` notice. A missing baseline for this machine fails.
After an intended change, or on a new machine or toolchain, record baselines
and commit them; that commit is the deliberate acceptance of the new cost:

```bash
python3 scripts/confidence.py --check build
python3 scripts/confidence.py --check perf-baseline
```

The UiO product runs its own scenarios with `python3 scripts/check.py perf`.

## Snapshots and public examples

Use `cargo insta review` to review intentional output changes. Keep snapshots
with the tests that exercise the behavior: spawned stdout/stderr in contracts,
terminal flows in e2e, local layout invariants in existing unit checks.

Doctests teach small, stable public entrypoints and copyable usage. The curated
`.public-api-examples.txt` baseline requires complete, nonempty runnable Rust
blocks; the existing doctest target verifies they compile and execute. Do not
inflate examples with private fixtures or state assertions to raise coverage.

## Reporting results

Report which existing checks passed, how you exercised the requested behavior,
and what remains unverified. Full CI checks the revision submitted for merge;
a local hook checks the working tree and may not match the revision pushed.

## Config internals and checks

Each value is tracked with its origin and scope:

- `config::LayerEntry` = raw value, scope and optional origin
- `config::ConfigValue` = typed value before or after resolution
- `config::ResolvedValue` = final value, raw value, source, scope and origin

Scopes are always optional. Unscoped values act as the baseline.

Minimum contract tests:

1. Profile scoping beats unscoped values.
2. Terminal scoping beats unscoped values.
3. Session overrides beat environment and config.
4. Placeholder cycles raise errors.
5. Unknown keys are rejected unless under `extensions.*`; retired keys warn and
   are ignored.

