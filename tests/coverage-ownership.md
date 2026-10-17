# Coverage ownership

Counts are textual `#[test]` declarations, including platform/Miri gates and
macro bodies, not assertion counts or independent terminal sessions. The source
pair is library `58fbfc0c` and product `8ecdd903`. The product is unchanged here.

Before: 1,058 declarations: 882 in source (402 inline, 480 in source test
files), 117 binary contracts, 21 integration
scenarios, 35 e2e scenarios, one external unit declaration, one architecture
check and one root/Miri declaration. After: 1,052 declarations: 866 in source
(402 inline, 464 in source test files),
126 contracts, 36 integration scenarios, 21 e2e scenarios, and the same unit,
architecture and Miri declarations. Cargo targets and platform gates are retained.
There are 25 snapshots before and after (four source, 21 contract); no snapshot
contents are changed. Runnable doctests are retained unchanged.

## Consolidated assertions

The destination scenarios passed before these old declarations were removed.

- `src/dsl/tests/parser.rs::parse_pipeline_preserves_quoted_pipe_in_command`:
  raw command `ldap user 'foo|bar'` and stage `P uid` now belong to
  `src/cli/tests/pipeline_parse.rs::parse_preserves_quoted_pipe_inside_command_segment`,
  alongside its retained unquoted command-token and stage assertions. The raw
  parser stays private; no public parsing API is reintroduced.
- `contracts/command_surfaces.rs::top_level_help_guide_contract`: auto-format
  help contains Usage and Commands →
  `top_level_help_supports_all_explicit_output_formats_contract` (same argv).
- `contracts/command_surfaces.rs::intro_command_expressive_guide_contract`:
  configured display name Demo, Commands and help →
  `intro_command_expressive_guide_preserves_user_context_and_section_order_contract`.
  It retains the same expressive/simple-prompt/name/theme configuration and
  also checks full welcome text, authentication, theme and section order.
- `contracts/command_surfaces.rs::intro_command_compact_contract`: compact
  anonymous intro contains invocation usage, Commands and help →
  `intro_command_compact_and_austere_diverge_by_presentation_contract`.
- `contracts/command_surfaces.rs::intro_command_austere_contract`: austere
  anonymous intro contains Welcome and Commands → that same comparison,
  which checks the complete versioned welcome line and absence of usage.
- `e2e/binary_surface.rs::binary_version_exits_zero_and_writes_stdout_only`:
  successful `--version`, exact version line and empty stderr →
  `contracts/version_commands.rs::cli_reports_workspace_version_with_long_flag_contract`.
  The short-flag contract now checks exact stdout and empty stderr too.

No deleted-feature declaration is removed in this change. No individual-bug
scenario is added, and no failure is used as a deletion justification.

## Boundary and module ownership

- Config: two file/env and scoped-list mutation workflows move intact from
  `src/config/tests/loading_and_mutation.rs` to `integration/config.rs`.
  Detailed explain/provenance and private credential-backend assertions remain
  in the config module because those APIs are private.
- App diagnostics: the interrupted product-manifest/repaired-manifest workflow
  moves from `src/config/tests/diagnostics.rs` to `integration/app/diagnostics.rs`.
  Its startup hook uses public `AppRuntime::config()` and loader resolution;
  unused private schema assembly is removed. Source labels, parser cause,
  rendered advice, exit status, exact JSON error envelope and repaired typed
  output assertions remain. This is an App extension workflow, not a schema unit.
- DSL/output: all 12 scenarios from `src/dsl/tests/integration/regressions.rs`
  move to `integration/dsl_output.rs`, using public output types and
  `apply_output_pipeline`. Collection preservation, grouping, filters, whole
  expression consumption, dates, mixed/scalar sorting, value order, jq and
  quick search assertions remain. Parser/verb-local invariants stay in source.
- Context: the two public session-sharing/redaction and native-command context
  scenarios move intact from `integration/app/host.rs` to `app/context.rs`.
- Process contracts: all seven plugin subprocess scenarios move intact from
  `e2e/plugin_processes.rs` to `contracts/plugin_processes.rs`; all remaining
  binary-surface assertions move to `contracts/binary_surface.rs`. Three
  one-shot profile/config-path/closed-pipe scenarios move from e2e CLI invocation
  to `contracts/cli_invocation.rs`. Shared bounded process/plugin factories
  move to `support/process.rs`; the PTY helper reuses them.
- REPL/history: the multi-process profile-scoped history capture/prune/clear/
  resume conversation remains intact in `e2e/history.rs`. Bang expansion,
  persistence parsing and shell-scope invariants stay with their REPL owners.
- Verbosity/diagnostics: `contracts/error_reporting.rs` owns stdout/stderr,
  JSON, advice and debug/forensic rendering. The moved App scenario owns
  diagnostic source chains. Existing local failure-classification tests remain.
- Plugins: discovery/help, enable/disable management and dispatch have
  separate contract slices. The six old helper tests named in the historical
  inventory were already absent at the starting revision. Cache reuse/refresh/
  executable invalidation, optional metadata, search precedence, inherited auth,
  runtime hints and describe timeout remain at process boundaries. Checksum/
  manifest helpers no longer exist. Provider selection has neither production
  machinery nor executable contracts at this revision, despite the retained
  DF-12 requirement. Its old snapshots are evidence of an unresolved ownership
  gap, not live coverage. Restoring the feature needs production work outside
  this scope. The current host's 64 MiB capture limit
  has no explicit boundary assertion; this change does not claim that coverage.

## Presentation

Contract child environments fix width/height at 80x24 and locale at C.UTF-8.
Snapshot scenarios explicitly choose render/color/unicode modes; local render
snapshots use explicit settings (including their 60/120-column variants).
Only line endings, log timestamps and scenario-supplied volatile paths are
normalized. Snapshot diffs require individual inspection; `.snap.new` files
are never accepted in bulk. Existing plugin snapshots with no remaining call
sites are retained for separate ownership reconciliation, not silently deleted.

## Verification

All commands use `CARGO_TARGET_DIR=/home/oisov/osp-work/target-w30-libtests`
and `CARGO_BUILD_JOBS=4`; the hermetic runner appends `osp-cli-rust` to its
artifact directory. Three normal-parallel full runs and one serial full run
passed: 1,132 executed tests each (1,052 ordinary tests plus 80 doctests), zero
failed/ignored. The first two used `confidence.py --check test`; the third used
`run-hermetic-cargo.py -- cargo test --locked --all-features --no-fail-fast`;
the serial run added `-- --test-threads=1` to that command.

Successful warm target times, excluding compilation, in run order:

- Source: 1.58 / 0.84 / 2.15 seconds; serial 9.65 seconds.
- Contracts: 4.96 / 3.21 / 7.40 seconds; serial 24.02 seconds.
- E2e: 15.24 / 15.23 / 15.35 seconds; serial 52.84 seconds.
- Integration: 5.07 / 12.49 / 7.59 seconds; serial 8.07 seconds.
- Doctest execution: 0.14 / 0.27 / 0.22 seconds; serial 0.83 seconds.

Cargo preparation/compilation took 9.38 / 3.73 / 0.33 seconds; serial 0.30
seconds. Concurrent local checks contributed load; these are observations,
not performance thresholds. Terminal scenarios dominate suite time. All three
parallel runs include plugin subprocess, prompt, completion and history lanes;
none showed ordering failures, poisoned-lock cascades or teardown timeouts.

Focused integration (36), pre-consolidation contracts (131), e2e (21) and the
quoted-pipe replacement (one) passed. Earlier compile attempts exposed private
config APIs and a missing local helper during relocation; both were corrected
before successful checks. There is no claim of a pristine pre-edit runtime
baseline. Final `confidence.py --check` checks `contract-env`, `public-docs`,
`public-api-examples`, `rustdoc-warnings`, `clippy` and `fmt` all passed, with
zero compiler/clippy warnings in the final checks. No scripts, dependencies,
production behavior, user-facing messages or downstream files are changed.
Production-line delta is zero; source registration edits are test-gated.

Focused successful scenario times, excluding Cargo preparation: plugin describe
limits 1.19 seconds, missing cursor reports 0.24 seconds, and profile-scoped
history capture/maintenance/resume 1.56 seconds (one passed each). Their Cargo
preparation took 0.11 / 0.18 / 0.18 seconds. These measurements do not attribute
the entire terminal target's runtime to any one scenario.
