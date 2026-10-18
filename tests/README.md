# Library tests

Run from the library root with `CARGO_BUILD_JOBS=6`, `RUST_TEST_THREADS=6`, `nice -n 15`, and a task-specific
`CARGO_TARGET_DIR`. Use `python3 scripts/confidence.py --check test` for the
full hermetic suite. Focused checks are `--check integration`, `--check contracts`
and `--check e2e`. Keep the runner's prerequisites and static/public-doc checks.

- `integration/`: public App/config/native-context workflows and DSL output
  semantics, with buffered sinks or explicit inputs. Process-based basic REPL
  and completion scenarios also live here.
- `contracts/`: binary argv, persisted config, plugin discovery/management/
  protocol, help, history and verbosity/diagnostic stdout/stderr/status.
- `e2e/`: terminal-dependent prompt, redraw, completion, help, color, plugin
  dispatch and profile-scoped history conversations. Unix PTYs are required.
- `src/**/tests*` and `#[cfg(test)]`: private parsers, local invariants and
  failure paths that add signal beyond public workflows. Public API narrowing
  is not a reason to move an otherwise public workflow into a private module.
- `support/`: isolated directories/environments, bounded process/PTY helpers
  and output parsing. `fixtures/` holds static data and the keyring harness.

Contracts normally run in parallel. Process-global environment scenarios use
`support/isolation.rs` to run the exact scenario in a private child with a
whole-scenario deadline and failure transcript. Do not serialize the suite to
hide leakage. PTYs use 80x24 geometry; children must be killed and reaped on
failure. The Linux keyring contract needs a private D-Bus session, installed
keyring tools and an unprivileged user; missing prerequisites are failures.
Use loopback fakes and synthetic credentials only.

Snapshots own human presentation at explicit width, C.UTF-8 locale and selected
color/unicode/theme settings. Parse JSON/TOML for semantic assertions. Inspect
snapshot diffs individually; never bulk-accept `.snap.new`. Preserve actual
output promises and normalize only irrelevant volatile values.

For a serial comparison, run:

```sh
python3 scripts/run-hermetic-cargo.py -- cargo test --locked --all-features --no-fail-fast -- --test-threads=1
```

Do not add individual-bug regression tests. Reuse existing behavior scenarios;
remove only mapped duplicates, private details already covered at a boundary,
or explicitly deleted features. Record every distinct removed assertion and
its retained owner in [coverage-ownership.md](coverage-ownership.md), which also
lists unresolved pre-existing coverage gaps.

For a focused edit/test loop, keep default test threads, use your own
`CARGO_TARGET_DIR`, and select the owning target plus an existing name filter.
Prefix each command with
`CARGO_BUILD_JOBS=6 RUST_TEST_THREADS=6 nice -n 15 python3 scripts/run-hermetic-cargo.py --`.
Times are for a one-line edit with a warm target:

| Changed | Command | Time |
| --- | --- | ---: |
| Private logic in `src/**` | `cargo test --locked --lib history_store` | ~3s |
| Public App/REPL workflow | `cargo test --locked --test integration repl::` | ~3s |
| Terminal behaviour | `cargo test --locked --test e2e history::` | ~5s |
| Before handing off | `cargo test --locked` | ~13s |

`--lib` does not build the `osp` binary. Integration, contract and e2e targets do.
Downstream app contracts also cover framework edits; see that repository's
`tests/README.md`. `cargo-nextest` works here too: 1052 tests in about 7-8s, with no
failures. It skips the 80 doctests, so the total is about the same as `cargo test`.
It stays optional.

The dev/test profiles retain line tables for local backtraces and omit dependency
debug information. The thin `osp` entrypoint has no unit tests; binary behavior
is exercised by the process and PTY contracts rather than an empty test harness.
