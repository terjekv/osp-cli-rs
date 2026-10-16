# Changelog

All notable changes to this project will be documented in this file.

The current release flow requires one versioned changelog section and one
matching `docs/releases/vX.Y.Z.md` file before a tag can be published.

## [Unreleased]

- **Breaking:** Add `FlagCompletions::positionals` for dynamic suggestions keyed
  by zero-based argument slot, including variadic slots. Omitted slots keep static
  suggestions; empty lists suppress them. Struct literals must supply the field
  or use `..FlagCompletions::default()`.

- Keep `-d` operator lines compact; include span context at `-dd` and above.

- **Breaking:** Native command factories leave clap grammars unbuilt so invocation help can
  set usage names before clap caches them.

- **Breaking:** Native metadata preparation receives `CommandScope` so recovery
  hints from runtime catalog discovery match the current prompt.

- Write follow-up commands and error hints for the current prompt: terminal
  suggestions include `osp`, REPL suggestions omit it, and integration shells
  use relative commands unless a host command or alias would shadow them.
  REPL JSON follows the same scope; one-shot JSON keeps its command spelling.
- Preserve command colour in relative REPL tables using the `command` field.
- Provider conflicts in the catalog state only that selection is required;
  invocation errors retain the selection command.


- Write help usage lines for the current terminal or REPL shell, including the
  `osp` executable in one-shot native help.

- **Breaking:** `RenderRuntime` gains a public `command_scope` field. Native
  command contexts now carry the reader scope for follow-up commands.

- Add `core::CommandScope` for command suggestions written for the current prompt.

- Default help lists `-v`, `-q` and `-d`; detailed help keeps the full
  invocation-option table.

- **Breaking:** Remove `ui.help.level = "inherit"`; the default is `normal`.
  Invocation `-v`/`-q` now adjust an explicit help level, and REPL `help -v`
  includes invocation options. Verbosity counts saturate safely.

- Plain detailed diagnostics use uncolored ASCII source annotations instead
  of the narrated screen-reader report.

- Startup failures now honor explicit mode, color and Unicode render flags
  even when configuration cannot load. Configured verbosity and debug bases
  also apply to startup diagnostics whenever configuration resolves.

- REPL failures retain plugin messages and use the one-shot failure renderer
  and usage-error titles. Show one detail hint only below detailed verbosity.

- JSON failure envelopes now stay identical across verbosity levels; requested
  failure explanations and backtraces go to stderr.

- **Breaking:** Failures use one detailed report at `-v` and above; `-vvv`
  saturates at `-vv`. Captured backtraces move to stderr at `-dd`, including
  `doctor last -dd`. Remove the public `ErrorDetail` API and UI field;
  `LastFailure::detail` and `render_report_message` take `MessageLevel`.
- Detailed failures show each cause and backend details once, and suppress
  empty result output on failure.

- `plugins doctor` warns about plugin flags consumed by the host; providers
  remain runnable.

- `-d` now records command dispatch and outcomes. CLI timing moves from the
  footer to the final log event; the REPL timing badge stays. Enabled file
  logs now default to `info` instead of `warn`.

- `-d` now shows plugin stderr on successful execution.

- REPL per-line `-d` flags now adjust stderr logging for that command only.

- `-d` now adds to the configured `debug.level`, saturating at trace level.

- **Breaking:** Inspect `LastFailure::detail` on demand instead of reading a
  pre-rendered detail field.

- **Breaking:** Render messages with `MessageRequest` and section dividers with
  `SectionDividerRequest` through their single rendering entry points.
- **Breaking:** Use `core::MessageLevel` for runtime verbosity and plugin response
  severity, replacing `UiVerbosity` and `ResponseMessageLevelV1`; wire and
  environment spellings are unchanged.

- **Breaking:** Render output through `ui::render` and `RenderRequest` instead
  of separate row, document, copy, and configured output render functions.

- Use one command output value with an exclusive structured, text, JSON, or
  empty body across CLI, REPL, progress, and replay.

- **Breaking:** Move progress and presentation signals from `OutputMeta` to
  command presentation state, independent of canonical output data.

- **Breaking:** Carry typed `OutputDocument::{Semantic, Json}` payloads instead
  of a kind and an internal JSON encoding of semantic documents.

- CLI help and REPL completion now share the same short descriptions.

- **Breaking:** Remove `CommandDef`, `ArgDef`, `FlagDef`, `ValueKind`,
  `ValueChoice`, their conversions, and `Document::from_command_def`. Native
  and builtin metadata comes from clap; plugin metadata stays at the wire boundary.
- **Breaking:** Fold `CommandSpec` into `CompletionNode`. Use `from_clap` or
  `from_describe`, and augment the node's `children` map directly. Duplicate
  child names are rejected during fluent construction rather than tree lowering;
  `CompletionTreeBuildError::DuplicateSubcommand` is removed.

- **Breaking:** Replace `ReplLineResult` and `ReplRunResult` with `ReplResult`.
  Use `Print(output)` to print and continue, or `Continue` for no output.
  `run_repl` returns only `Exit` or `Restart`.
- **Breaking:** Move completion-debug menu fields into `CompletionDebug::menu`
  (`MenuDebug`). The `repl debug-complete` JSON keys remain unchanged.
- **Breaking:** Remove `NativeSessionContext::set_context_value_with_prompt`;
  use `set_prompt_value(key, label, value)` instead.

- **Breaking:** Native catalogs return `CommandCatalogEntry`, whose `source`
  explicitly distinguishes native commands from plugins.
- **Breaking:** Replace mutable discovered-plugin command vectors with one
  normalized command map.

- Add exclusive product action flags with an optional config argument and
  sink-rendered confirmation, for flows such as explicit product login.
- Configure access policies from resolved config before help and runtime
  visibility; apply product visibility to root help as well.
- Support visibility blocks in intro templates and tips, and expose actual
  REPL startup separately from terminal-scoped presentation configuration.

- **Breaking:** Replace provider and placement completion metadata with the
  generic `DynamicCompletion` callback on `CompletionNode`.
  Remove `Planning*`, `OsVersions`, `RequestHints`, `RequestHintSet`,
  `ProviderNarrowing`, `narrow_provider_candidates`, and provider fields on
  `FlagNode` and `FlagHints`. Products now own their contextual completion rules.
- **Breaking:** Remove test-only completion constructors and analysis helpers
  from the public API, including `CompletionNode::{with_child,with_flag}`,
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

- **Breaking:** Replace `StartupHook` and `CommandAccessRecovery` with
  `App::with_access_policy` (`AccessPolicy`) for product visibility, startup,
  execution preparation and live session facts. Remove recovery requests and
  outcomes. The hook survives REPL rebuilds and runs before native metadata
  preparation; help, completion and builtins do not acquire credentials.
- **Breaking:** Remove builtin-policy builders, policy contexts, registries,
  overrides and the mirrored command-policy DSL. `CommandAccess` is `Runnable`, `Denied(reason)` or `Hidden(reason)`.
  Remove the unused `CredentialState::present()` constructor.
- **Breaking:** Remove config keys `auth.visible.builtins` and
  `auth.visible.plugins`; configs that set them now fail as unknown keys.
- **Breaking:** Remove capability, profile, product-availability, feature,
  visible-session, auth-strength and credential-freshness gates. Protocol-v1
  DTOs still parse the legacy fields and ignore them; `capability_gated` is
  public. Plugins can still require authentication or present/valid credentials.
- **Breaking:** Doctor session JSON drops `auth_strength`, `capability_count`
  and `enabled_features`. Plugin command listings drop `required_capabilities`,
  `feature_flags` and `visible_session`; run requirements and hints show only
  supported fields. Credential validity and TTL diagnostics remain live.
- Build native help from clap metadata and refresh native invocation metadata
  at most once, with one linear parse, access, refresh and execution path.

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
