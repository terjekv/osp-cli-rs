# Library tests

Run from the library root with `CARGO_BUILD_JOBS=4` and a task-specific
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
