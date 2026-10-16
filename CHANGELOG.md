# Changelog

All notable changes to this project will be documented in this file.

The current release flow requires one versioned changelog section and one
matching `docs/releases/vX.Y.Z.md` file before a tag can be published.

## [Unreleased]

- **Breaking:** Replace provider and placement completion metadata with the
  generic `DynamicCompletion` callback on `CommandSpec` and `CompletionNode`.
  Remove `Planning*`, `OsVersions`, `RequestHints`, `RequestHintSet`,
  `ProviderNarrowing`, `narrow_provider_candidates`, and provider fields on
  `FlagNode` and `FlagHints`. Products now own their contextual completion rules.
- **Breaking:** Remove test-only completion constructors and analysis helpers
  from the public API, including `CompletionNode::{sort,with_child,with_flag}`,
  `CursorState::synthetic`, `Suggestion::new`, `CommandLine::last_flag_occurrence`,
  `CompletionEngine::{analyze_command,matched_command_len_tokens}`, and
  `CommandLineParser::{analyze,compute_stub_quote}`,
  `FlagNode::{multi,value_type}`, and `SuggestionEntry::{display,aliases}`.

- Share scoped editing between config set and unset while preserving session,
  TOML, secret-backend, dry-run, explain, and message behavior.

- Build config explain facts once and format text from the same redacted data
  emitted as JSON, preserving both output contracts.

- **Breaking:** Remove render settings/runtime builders. Construct render
  settings and runtime facts directly.

- **Breaking:** Remove test-only config resolver, layer, schema, and loader
  mutation methods. Resolve prepared `LoadedLayers` with an explicit schema.

- **Breaking:** Remove the unused config keys `session.cache.max_results`,
  `ui.prompt`, `ui.prompt.secrets`, and `base.dir`. Configurations containing
  these keys now fail as unknown keys.
- **Breaking:** Remove `ui.help.table_chrome`, `ui.help.entry_indent`,
  `ui.help.entry_gap`, `ui.help.section_spacing`, `ui.medium_list_max`,
  `ui.indent`, and `repl.prompt_right`. Their former default behavior is fixed;
  configurations containing these keys now fail as unknown keys.

- Declare builtin config schema, defaults, documentation, and bootstrap rules
  together.

- REPL completion now includes every invocation alias, including `--no-color`.
  The verbose overview consistently lists each flag with all its aliases.
- `--format` help and completion list its values in the same order as the
  config schema: `auto|json|table|md|mreg|value`.
- Startup diagnostics now use the same invocation options and verbosity as
  dispatch. Only complete `-vqd` clusters count; option values, unknown short
  flags and later DSL stages no longer contribute verbosity or render overrides.
  Conflicting Unicode flags follow argument order when rendering their error.

- **Breaking:** Remove the unused `Cli::bootstrap_from` preflight parser.

- **Breaking:** Remove `osp_cli::guide` and its guide view, section, and entry
  types. Use `core::document::Document` and semantic blocks instead; guide
  restoration and `OutputDocumentKind::Guide` are removed. Semantic hints use
  `OutputDocumentKind::Document` and `RenderRecommendation::Document`; output
  metadata carries explicit document-row identity and presentation hints.
- **Breaking:** Remove the `guide` output format, `OutputFormat::Guide`, and
  `--guide`. `ui.format = guide` is invalid, and plugin `OSP_FORMAT` hints no
  longer include `guide`. Use `auto` for human document rendering.
- **Breaking:** Remove the `ui.guide.default_format` config key and
  `GuideDefaultFormat`; it no longer had any effect. Remove it from config
  files, where it is now an unknown key.
- **Breaking:** Unstaged help and intro JSON now emit a `blocks` object. Pipes
  emit flat rows carrying `section` and entry descriptions; sorting and limiting
  retain their ordinary row meanings and never reconstruct a help envelope.
- Build builtin help from clap metadata and retain external help continuations.
  Intro templates preserve nested and loose Markdown lists.

## [1.5.1] - 2026-03-17

- Stabilizes the internal host, UI, config, and REPL architecture with a broad behavior-preserving cleanup that centralizes ownership without changing the public release shape.
- Tightens rustdoc, release metadata, and local gate discipline so public API examples, release checks, and coverage/readiness contracts stay aligned with the crate surface.
- Removes low-signal duplicate tests, restores changed-file coverage on the app host facade, and keeps the full test and coverage gates green after the internal reorganization.

## [1.4.5] - 2026-03-08

- First real public binary release of the Rust `osp` CLI, aligned to the current `osprov-cli` version line.
- Ships the rebuilt UI and REPL model with explicit `expressive`, `compact`, and `austere` presentation profiles.
- Enforces release verification, coverage gating, versioned release notes, and changelog-backed tagging.

## [1.4.6] - 2026-03-08

- Promoted the root package to the canonical single-crate implementation and removed the mirrored `osp_*` modules from the active build.
- Compiled presentation presets into canonical config/UI keys so REPL and help behavior stop relying on ad hoc fallback logic.
- Finished the crates.io release path with trusted publishing while keeping cross-platform GitHub release artifacts.

## [1.4.7] - 2026-03-08

- Adds a generic upstream command-policy layer with typed visibility, runnability, policy registry, and access evaluation so downstream distributions can supply auth facts without reimplementing policy mechanics.
- Extends plugin describe metadata, plugin listings, and REPL/help surfaces with generic auth metadata and compact auth hints while keeping capability semantics downstream-owned.
- Tightens upstream dispatch and runtime integration so command policy is enforced consistently across discovery, UI surfaces, and execution, with coverage kept above the repo push gate.

## [1.4.8] - 2026-03-09

- Adds the first-class native top-level command registry so downstream products can register commands like `osp ldap` cleanly through upstream help, completion, REPL, and dispatch.
- Removes the long-stale legacy `workspace/` mirror and updates release/test tooling to reflect the single-crate layout.
- Stabilizes the release verification path by fixing clippy noise in native command outcomes, hardening brittle fixture tests, and lowering the changed-file coverage floor to `85%`.

## [1.4.9] - 2026-03-11

- Tightens the public Rust API around canonical entrypoints and guided construction so embedders have one documented path for builders, constructors, and runtime setup.
- Centralizes host assembly and runtime planning so startup, rebuild, plugin activation, rendering, and REPL coordination stop re-deriving the same policy in multiple places.
- Rebuilds the intro/help document path around semantic `osp` template blocks, ordered guide sections, and shared ruled chrome so REPL startup and help output keep the intended structure across rich, compact, ASCII, and JSON surfaces.
- Fixes quick-search and structural DSL restore semantics so filtered help/intro payloads keep only the intended branches without resurrecting stale canonical buckets or leaking unrelated siblings.
- Replaces the `JQ` DSL verb's external `jq` subprocess dependency with an embedded `jaq` evaluator so the pipeline stays stable in-process while keeping the familiar `JQ` user surface.
- Consolidates architecture, contract, integration, end-to-end, and coverage-gate tests so failures localize faster while duplicate happy-path coverage drops below the old churn-heavy baseline.

## [1.5.0] - 2026-03-13

- Tightens the public API around the real long-term seams: typed config-store edit options, `App::builder()` as the canonical host front door, wrapper-owned product defaults, and a clearer guided-construction story for embedders.
- Removes retired DSL rollout compatibility shims, hardens runtime and completion paths against panic-style failure, and adds regression guards that keep the non-test runtime free of `panic!`, `unwrap`, and `expect`.
- Keeps explicit JSON output machine-readable across headless and PTY execution by forcing plain rendering on JSON surfaces, and hardens the local merge gate by running contract and integration lanes under a hermetic runtime environment.
- Rebuilds the rustdoc and repo docs experience around credible entrypoints, wrapper guidance, and runnable examples, including a minimal product-wrapper example crate that shows how downstream teams should inject native commands and product defaults.
