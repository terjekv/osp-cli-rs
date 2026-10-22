# Changelog

All notable changes to this project will be documented in this file.

The current release flow requires one versioned changelog section and one
matching `docs/releases/vX.Y.Z.md` file before a tag can be published.

## [Unreleased]

### What's new in v2

- Add commands by dropping an `osp-NAME` executable on PATH. Optional
  `--osp-describe` metadata adds help, completion and JSON/DSL integration.
  First-found plugins win; terminal input and stderr are inherited.
- Share selected entities across the REPL. Reads announce remembered inputs;
  writes require an explicit value or `.`. Scoped selections can persist.
- Search history with Ctrl-R, edit global recalls and preview bang expansions.
  A leading space keeps a command off disk.
- Use cumulative `-v`, `-vv` and `-vvv` for command evidence, independently of
  host debug flags. Errors stay concise; `doctor` checks local setup only.
- Explore every visible command with `help --all`, pipe the last result locally,
  run `each`, export CSV/TSV and filter relative times.
- Embedders use direct `App` configuration, a narrower public API, shared context
  and the `AccessPolicy` lifecycle. Prompting follows terminal and output rules;
  global history selections bypass the current shell.
- Retired config keys warn and are ignored. See the migration notes below for
  plugin protocol/argv changes, rendering APIs and removed settings.

### Changes

- Summarize multi-field object arrays and objects in terminal table cells as
  item or field counts. Protect counts during fitting and include them in the
  shortened-values footer with a card action; cards and exports keep details.

- Render compact cards on one aligned line per record when every title and
  nonempty field fits at the target width; otherwise retain titled cards.

- Size each compact grid value column from its own rendered values across
  records, with two spaces after labels and three spaces between pairs.

- Indent compact card fields and wrapped continuations two columns under
  double-rule titles, including the indent in width planning.

- Limit the singleton card override to Table recommendations; other recommended
  formats retain precedence for single records.

- Combine screen table hidden-column and shortened-cell summaries with concise
  pager, card and quoted column-pick actions. Wrap the footer instead of cropping
  it; nested card tables omit actions. Quiet/help-none output suppresses it.

- Fit list table cells between items with a remaining `+N` count. Only an
  overwide first item is ellipsized; wrap mode prints whole items on separate
  lines. Lists shrink as prose instead of protecting the joined text as a token.

- Reduce nested single-field object arrays to value lists for human rendering
  after removing uniformly empty fields. These scalar lists qualify for compact
  cards; nested tables also omit empty fields. Exports retain canonical data.

- Align compact card grid values and long-field values in one label column
  sized across the whole result.

- Prefer classic cards for one ordinary record, including table-recommended
  results. Explicit and configured formats take precedence over recommendations.

- Honour explicit colour and Unicode choices on screen, pager and pipe targets;
  automatic pipe rendering stays plain. Editor and clipboard suppress colour.

- Add producer-declared colour swatches. Coloured `theme list` shows two-cell
  blocks measured before fitting; plain output and exports keep raw colours.

- Choose eligible automatic table/card layouts by visible values per rendered
  line, keeping tables on ties. Hidden columns alone no longer force cards.

- Use compact ruled cards for multi-row flat MREG results: shared short-field
  grids, wrapped long fields and no blank card separators. Single and nested
  records retain the classic layout; title-only records use labelled entries.

- Render human single-column row lists as titled, width-aware grids. Explicit
  MREG prints consecutive labelled entries; explicit tables and Markdown remain
  tables, while machine and Value outputs retain their existing representation.

- Preserve complete atomic table values (tokens up to 24 columns) and the first
  column. Shrink prose columns and drop trailing columns with a hidden footer
  instead of truncating identifiers, colours or addresses.

- Resolve width, colour, margin and display limits by render target. Margin is
  REPL screen chrome only, including JSON. One-shot output and sinks have no
  margin. Pager/editor/clipboard actions re-render retained structured results
  without terminal truncation or display limits; editors suppress colour and
  clipboard stays ASCII. Plain-text commands retain a text fallback.
  One-shot automatic paging remains; the REPL and machine formats never
  auto-page. Default to `less -RS` when `PAGER` is unset.

- Plan every output format before dispatch. Configured CSV/TSV preserve help's
  guide layout by default, recommended exports emit records, and JSON writes
  canonical typed values directly as plain JSON. Auto recommendations defer
  to data inference; machine exports never auto-page.

- Add `ui.table.too_wide=mreg|hide` (default `mreg` in every preset). Eligible
  automatic TTY tables become cards only at higher value-per-line density;
  explicit/configured tables retain the hide path. See `docs/UI.md` for guards.
  Explicit MREG always requests cards, including flat lists and grouped rows.

- Project human data once before layout. Derived totals use unlimited input;
  newline cells are measured as rendered. Empty declared tables retain headings.
  Human formats localize RFC3339 strings; Value and machine output keep raw
  timestamp values.

- Require producer roles for usage/options, command styling and document rows.
  Entry-shaped data remains data; JSON retains its existing content schema and
  omits presentation roles. Carry token/spec styles on the actual projected
  cells, fixing nested/grouped palette-source mixups and styling MREG values.

- Share title/rule geometry across documents, messages and completion: default
  width 24, minimum 12, normalized whitespace/colons and no trailing title space.
  Remove the test-only help shim; existing checks use the production help path.

- Narrow the alpha rendering API: remove unused `StyleOverrides::mreg_key` and
  `json_key` and make byte formatting internal. Keep the public local and
  relative timestamp helpers. Export document roles and `ui::TableTooWide` for typed
  producers and embedders.

- Cycle distinct inline history suggestions with Ctrl-Right at the end of
  input while the menu is closed. Editing or moving the cursor resets cycling;
  Right/End still accept the full suggestion and Alt-Right accepts one word.

- Rank inline history suggestions within the last 200 prefix matches. Recency
  wins unless another command is at least three times as frequent; ties retain
  recency, and suggestions still agree with unique command-tree completions.

- Add `repl.autosuggest` (default `true`) and `with_autosuggest` on the public
  run-config builder. Disabling it hides inline text while retaining status
  and help guidance below the input.

- Replace `repl.tab_mode` and the public `ReplTabMode` API with the trigger list
  `repl.complete_on` and `ReplCompleteOn`. The run-config builder now exposes
  `with_complete_on`; defaults open on flag starts, and an empty list leaves
  only explicit completion.

- Reject known injected secrets in context selections and announcements; redact
  them from context provenance while preserving canonical command result data.

- Expand context `.` values in attached options and short option groups, and
  retain the expanded or filled invocation when producing context selections.

- Merge persisted context changes under a file transaction so concurrent REPLs
  preserve other identities’ selections and use unique atomic writes.

- Finish captured plugin runs when the direct child exits, draining available
  stdout without waiting for descendant-owned pipes.

- Reject plugin describe metadata with colliding argument IDs, invalid flag
  aliases or unsupported positional layouts. Inventory reports the error while
  the executable remains available as a plain drop-in, including for old caches.

- Require `each --yes` when any template word is a write verb, including words
  after options or placeholders and restoration, assignment and access changes.

- Exclude REPL commands carrying sensitive named arguments from history and last
  command recall, including quoted templates, aliases, expanded commands and
  runtime failures.

- Prepare native metadata for explicit help on cold command paths. If no cached
  help exists, retain the provider's recovery advice when discovery fails.

- Treat `passwd` and `credential` names as sensitive alongside passwords, tokens,
  secrets and API keys wherever the shared sensitivity policy is used.

- Keep root help available when config or secrets TOML cannot be loaded, without
  calling product visibility policies before they have been configured.
  Concise TOML errors retain the file, line and column even when the parser
  message spans multiple lines; secrets source text remains hidden.

- Ignore retired config keys with one warning listing their names and removal
  guidance. Unknown keys still fail; see the config migration list below.

- `help --all` lists every visible command path as pipeable rows in CLI and REPL.

- Terminal confirmations remain available in one-shot commands. `may_prompt()`
  checks stdin/stderr terminals, JSON output and output pipelines; credential
  acquisition retains its own command-scope restrictions.

- Confirm successful `| Y` clipboard copies with the row or group count.

- Default history exclusions use `history*`, covering every history command
  and its flag variants.

- In a shell, Ctrl-R inside history search widens to all entries in the same
  identity/profile file, preserving the query. Prompts distinguish the shell
  from `all`. Global selections run outside the shell for that line; Tab keeps
  that scope while editing. Ctrl-R at root keeps its scope.

- Global history selections dispatch outside the current shell through the
  internal `ReplEvent::RootLine` event.

- History bang words are valid in the REPL highlighter; `!!` and `sudo !!`
  show their expansion as a muted hint without an unknown-command status.

- REPL commands ending in a usage error are excluded from saved history and
  the last accepted command, matching parse-error behavior. Runtime failures
  remain recallable.

- A leading space keeps a REPL line out of persisted history, including bang
  replays. The editor still allows one temporary recall with Up.

- Ctrl-R history search uses `history> `: Enter runs the selected command,
  Tab puts it in the editor, and Escape restores the draft.

- Native contexts expose terminal prompt eligibility independently of command
  scope. Confirmation input helpers leave consent policy with the caller.

- Redact sensitive names consistently in config, plugin environment values and
  error details, including cookies, authorization and underscore-delimited keys.

- REPL prompt borders honour Unicode suppression. Products can supply the
  greeting name with `AppRuntime::set_display_name` without changing identity.

- `DisplayRule::Hyperlink` marks HTTP(S) URL fields for terminal hyperlinks.
  The renderer alone emits OSC 8 in rich TTY output; plain output, exports,
  pipelines and `last` replay retain the plain URL. Product text remains sanitized.

- Alt+Enter inserts the most recent structured Run hint into the REPL input;
  Enter still decides execution. Configure `repl.bindings.hint.insert` with
  `Alt+Enter`, F2..F12 or `none`. Native commands can retain command hints and
  history entries independently of resource context.

- Keep a disabled REPL introduction disabled when verbosity increases. Host
  progress measures rendered height rather than asking native commands to
  estimate it. Retained failure command labels omit arguments.

- Restrict diagnostic logging to host/service events. Preserve structured plugin
  UI text with terminal sanitization and redaction of injected secrets.

- Ordered DSL filters accept checked relative times such as `now-24h`.

- Add `--csv` and `--tsv` output with ordered headers and compact nested JSON.

- Add REPL `each 'CMD {}'` over saved values, with `--yes` and `--ignore-errors`.

- Pasted `osp COMMAND …` REPL lines become editable commands before execution.

- REPL lines beginning with `|` pipe the saved answer without another request.

- Commands declare consumed arguments and singleton result fields through
  `ContextContract` on clap commands, including derive and generated grammars.
  Reads fill omitted arguments with an info message and JSON context metadata;
  writes use only explicit values or echoed `.` expansion. Filled arguments
  are retained in REPL history. Opt-in persisted kinds load before the prompt
  and never supply one-shot arguments.

- Plugin descriptions declare `produces` and `consumes`. Declared selections
  are passed as `OSP_CTX_<KIND>` environment variables in the REPL.

- Plugins may return bare JSON arrays or objects. Explicit format flags and DSL
  stages capture stdout for the existing renderer; text remains text, while
  DSL stages require JSON. Captured stdout retains the 64 MiB limit.

- Fix `ResponseV1` decoding to accept omitted `meta`; messages, context, and
  render hints remain supported. Plugins also receive effective `OSP_USER`.

- Rebuild native completion at safe prompt boundaries after background refresh
  and cold discovery, including subsequent execution or rendering failures.

- Hold foreground REPL notices until command output finishes, so the editor's
  next redraw cannot erase them.

- Add master-controlled terminal bell requests and `notifications preview` in
  rich human REPL sessions, with a ten-second limit.

- Native products can register suspended editor actions and cancellable REPL work.
  Background notices are bounded and pause during foreground terminal use.

- Native products can contribute sanitized cached status to the REPL right prompt.

- Add `AccessPolicy::before_dispatch` for product credential acquisition before
  native parsing, explicit help, metadata preparation, or REPL shell entry.
  Root help, completion and background refresh remain side-effect free.

- Human display paths accept JSON Pointers for literal field segments in
  columns, formatting rules and timestamp fields. Duplicate display labels
  retain separate cells with disambiguated headings.

- Keep `-d` operator lines compact; include span context at `-dd` and above.

- Write follow-up commands and error hints for the current prompt: terminal
  suggestions include `osp`, REPL suggestions omit it, and integration shells
  use relative commands unless a host command or alias would shadow them.
  REPL JSON follows the same scope; one-shot JSON keeps its command spelling.
  Help usage includes `osp` in terminal runs and follows the active REPL shell.

- Preserve command colour in relative REPL tables using the `command` field.

- TOML failure summaries include line and column without requiring verbose
  source annotations.

- Minimal REPL intros list visible command roots and aliases instead of
  subcommands and theme completion words.

- Default help lists `-v`, `-q` and `-d`; detailed help keeps the full
  invocation-option table.

- Startup failures now honor explicit mode, color and Unicode render flags
  even when configuration cannot load. Configured verbosity and debug bases
  also apply to startup diagnostics whenever configuration resolves.

- REPL failures retain plugin messages and use the one-shot failure renderer
  and usage-error titles.

- JSON failure envelopes now stay identical across verbosity levels; requested
  failure explanations and backtraces go to stderr.

- Command evidence saturates at `-vvv`. Failures keep concise
  error/request-id/hint wording; requested evidence and backtraces go to stderr.

- Suppress empty result output on failure.

- Use `doctor plugins` to inspect disabled, shadowed or invalid metadata.

- `-d` now records command dispatch and outcomes. CLI timing moves from the
  footer to the final log event; the REPL timing badge stays. Enabled file
  logs now default to `info` instead of `warn`.

- Stdin and stderr are inherited. Stdout is inherited unless structured
  rendering or a pipeline requires capture.

- REPL per-line `-d` flags now adjust stderr logging for that command only.

- `-d` now adds to the configured `debug.level`, saturating at trace level.

- CLI help and REPL completion now share the same short descriptions.

- Add exclusive product action flags with an optional config argument and
  sink-rendered confirmation, for flows such as explicit product login.

- Configure access policies from resolved config before help and runtime
  visibility; apply product visibility to root help as well.

- Support visibility blocks in intro templates and tips, and expose actual
  REPL startup separately from terminal-scoped presentation configuration.

- REPL completion now includes every invocation alias, including `--no-color`.
  The verbose overview consistently lists each flag with all its aliases.

- `--format` help and completion list its values in the same order as the
  config schema: `auto|guide|json|csv|tsv|table|md|mreg|value`.

- Startup diagnostics now use the same invocation options and verbosity as
  dispatch. Only complete `-vqd` clusters count; option values, unknown short
  flags and later DSL stages no longer contribute verbosity or render overrides.
  Conflicting Unicode flags follow argument order when rendering their error.

- Retain the public `guide` output format and `ui.guide.default_format`
  configuration using the shared document renderer.

### API and protocol migration

- **Breaking:** Remove final-text overrides from response metadata and render
  requests, together with the string-only progress append field.
  Final native/plugin output is structured `data` plus renderer hints; notices
  use levelled `messages`. Native streaming endpoints call `emit_stream` with
  object events: `--json` writes event JSONL to stdout followed by the final
  result document, and DSL stages see those same events. Optional structured
  terminal snapshots use `with_frame`; preliminary `emit_progress` stays on
  stderr. Removed metadata has no compatibility path.

- **Breaking:** The embedding API now exposes one canonical path for shared
  types and keeps host engines and state internal. App setup, native-command
  hooks, rendering/DSL entry points, config bootstrap/editing, and drop-in
  plugins retain their behavior. Product wrappers must update imports and
  runtime reads together with the library. See the response-metadata removal
  above for the wire protocol change.

| Previous API | Current API |
| --- | --- |
| `command_policy::*` | `core::command_policy::*` |
| `core::{output,output_model,runtime,row,command_scope,document}::*` | Flat `core::*` types, including semantic documents |
| `core::fuzzy` matchers, env constants, value helpers, `quote_for_shell` | Internal; supported helpers remain `core::{fold_case,escape_for_shell}` |
| `core::command_policy::CommandPath` | Internal host lookup key |
| `completion::{model,engine,parse,suggest,tree}::*` | Flat `completion::*`; analysis details are internal |
| `Cli.command`, CLI argument enums and parsers | `cli::Cli::has_command()`; parse `cli::Cli` through clap |
| `Cli` / `NativeCommandContext` struct literals | clap parsing / `NativeCommandContext::new()`; both are non-exhaustive |
| `AppRuntime::config_state().resolved()` | `AppRuntime::config()` |
| `AppRuntime::context().is_repl()` | `AppRuntime::is_repl()` |
| `AppRuntime::ui().render_settings.runtime.command_scope` | `AppRuntime::command_scope()` |
| Other host state getters, `AppState*`, session/client builders | Internal; configure the host through `App::new().with_*()` |
| Free `app::run_process_with_sink` | `App::run_process_with_sink` |
| Plugin manager and registry catalogs | Internal; register native commands or use the plugin wire protocol |
| Config schema, explanation model, `RuntimeConfig`, secret-store internals and default constants | Internal; use public loaders, `ResolvedConfig`, runtime bootstrap and TOML editing helpers |
| `ui::section_chrome::{SectionFrameStyle,RuledSectionPolicy}`, `ui::style::StyleOverrides` | Flat `ui::{SectionFrameStyle,RuledSectionPolicy,StyleOverrides}` |
| UI theme, message, clipboard and resolved-settings engines | Internal; use `ui::render`, `RenderSettings`, and `RenderRequest` |
| DSL parsing and verb metadata | Internal; use `apply_pipeline`, `apply_output_pipeline`, or `execute_pipeline` |
| REPL engine, history and debug APIs | Internal; use `App` and the native-command REPL hooks |
| `context::KINDS` | Internal; public context contracts, resolution and session APIs remain available |

- **Breaking:** Remove history record metadata and the profile-scoping toggle.
  Identity/profile isolation comes from `repl.history.path`; use a path without
  `${profile.active}` to share history across profiles. Old records still load
  and are rewritten as command/timestamp pairs on the next write. Consecutive
  duplicate suppression compares the last entry in the file across shell scopes.
  Default filenames lowercase the effective identity, preserving separate ordinary
  and drift histories. If the lowercase file is absent, an old mixed-case filename
  is read until the first write creates the lowercase file; custom paths stay literal.

- **Breaking:** Remove the unused `repl debug-complete --menu history` diagnostic
  and history-menu debug APIs. Ctrl-R continues to use the live history picker.

- **Breaking:** Command errors carry recovery actions and request IDs as data.
  Text prefixes such as `hint:`, `Try:` and `tip:` are no longer parsed into advice.
  Structured advice survives native clap error handling.

- **Breaking:** Native and inline commands share help/version/missing-subcommand handling.
  Missing subcommands show help successfully, including direct native LDAP
  execution.

- **Breaking:** Native preliminary notices and measured frames are suppressed for
  JSON, output pipelines and non-terminal stderr. Ordinary progress events
  retain their stream behavior.

- **Breaking:** `NativeCommand::repl_status` returns `ReplStatus` with semantic
  emphasis. Product text still strips terminal controls; error and warning
  emphasis uses the active theme and honours colour suppression.

- **Breaking:** `ReplAction.key` uses `ReplActionKey` to represent function
  keys and Alt+Enter.

- **Breaking:** `config get` rejects a missing explicitly selected config file
  instead of silently returning defaults.

- **Breaking:** Add command detail level `evidence` above `trace`, with useful
  cumulative `-v`, `-vv`, and `-vvv` service evidence. Native handlers receive a
  command-local diagnostic sink; plugins receive `OSP_COMMAND_VERBOSITY`.

- **Breaking:** Move service exchanges from developer logging to command
  verbosity and human supplements to stderr. CLI and REPL failures use concise
  `error:`, optional `request-id:`, and `hint:` lines; JSON actions live in
  `error.details.action`. Doctor reports local setup without remote probes.

- **Breaking:** Entity selections use one service/profile/identity-scoped store.
  All entries appear in `context` and the prompt; `context set KIND VALUE` pins
  a value and `context clear [KIND]` also clears its persisted copy. Remove
  `set_prompt_value`, `remove_prompt_value`, and separate prompt labels.

- **Breaking:** Plugins are drop-in `osp-NAME` executables. Builtins and native
  commands win name clashes; the first plugin found wins. PATH is searched by
  default. Root help now lists plugins, and inventory shows shadowed entries.

- **Breaking:** Replace `--describe` and the multi-command `DescribeV1` wrapper
  with optional `--osp-describe` returning a single command object without
  `name`. Include the literal marker in the executable to opt into discovery.
  Set `"output": "json"` to request automatic rendering. Describe failures are
  listed but do not block execution. Remove bundled `manifest.toml` files.

- **Breaking:** Plugin argv no longer includes a leading command name. Read
  `OSP_COMMAND` instead, using symlinks for multiple names from one executable.
  Runs inherit stdin/stdout and the full environment, have no timeout, and preserve
  exit codes. `extensions.plugins.timeout_ms` now limits describe only.
  `--help` and `-h` always use the executable's own parser and terminal output.

- **Breaking:** Remove `--plugin-provider`, provider selection and the retired
  provider config (listed below). Remove `plugins commands`, `config`,
  `doctor`, `clear-state`, `select-provider`, and `clear-provider`; use
  `plugins list`, `config get extensions.plugins`, and `doctor plugins`.
  Enable/disable now edit `extensions.plugins.disabled = ["name"]` in the
  active profile with `osp plugins enable|disable NAME`.

- **Breaking:** Rename `extensions.plugins.<id>.env.*` to
  `extensions.plugins.<command>.env.*`. Environment normalization collisions
  now use the later config value, with per-command values after shared values.

- **Breaking:** Configure hosts directly with `App` setters. Replace `AppBuilder`
  and sink-bound `AppRunner` calls with `App::new()` and explicit sink arguments.

- **Breaking:** Add `DisplayRule::HideArrayItemsWhenTrue` for human-only
  filtering of boolean-marked array members. Canonical JSON and DSL data
  retain every member; exhaustive matches on `DisplayRule` need the new arm.

- **Breaking:** Add `FlagCompletions::positionals` for dynamic suggestions keyed
  by zero-based argument slot, including variadic slots. Omitted slots keep static
  suggestions; empty lists suppress them. Struct literals must supply the field
  or use `..FlagCompletions::default()`.

- **Breaking:** Native command factories leave clap grammars unbuilt so invocation help can
  set usage names before clap caches them.

- **Breaking:** Native metadata preparation receives `CommandScope` so recovery
  hints from runtime catalog discovery match the current prompt.

- **Breaking:** `RenderRuntime` gains a public `command_scope` field. Native
  command contexts now carry the reader scope for follow-up commands.

- **Breaking:** Rename the message verbosity key to `ui.verbosity` (environment:
  `OSP__UI__VERBOSITY`); the retired key is listed below. Host UI and logging
  state are internal.

- **Breaking:** Remove the public `ErrorDetail` API and UI field. Failure
  rendering and retained failure state are internal; use host error presentation.

- **Breaking:** Use `core::MessageLevel` for runtime verbosity and plugin response
  severity, replacing `UiVerbosity` and `ResponseMessageLevelV1`; wire and
  environment spellings are unchanged.

- **Breaking:** Render output through `ui::render` and `RenderRequest` instead
  of separate row, document, copy, and configured output render functions.

- **Breaking:** Move progress and presentation signals from `OutputMeta` to
  command presentation state, independent of canonical output data.

- **Breaking:** Carry typed `OutputDocument::{Semantic, Json}` payloads instead
  of a kind and an internal JSON encoding of semantic documents.

- **Breaking:** Remove `CommandDef`, `ArgDef`, `FlagDef`, `ValueKind`,
  `ValueChoice`, their conversions, and `Document::from_command_def`. Native
  and builtin metadata comes from clap; plugin metadata stays at the wire boundary.

- **Breaking:** Fold `CommandSpec` into `CompletionNode`. Use `from_clap` or
  `from_describe`, and augment the node's `children` map directly. Duplicate
  child names are rejected during fluent construction rather than tree lowering;
  `CompletionTreeBuildError::DuplicateSubcommand` is removed.

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

- **Breaking:** Remove render settings/runtime builders. Construct render
  settings and runtime facts directly.

- **Breaking:** Remove test-only config resolver, layer, schema and loader
  mutation methods. Use `config::build_runtime_pipeline` for host loading;
  custom layers use `config::ConfigResolver` with the host-owned schema.

- **Breaking:** Remove the unused `Cli::bootstrap_from` preflight parser.

- **Breaking:** Replace `StartupHook` and `CommandAccessRecovery` with
  `App::with_access_policy` (`AccessPolicy`) for product visibility, startup,
  execution preparation and live session facts. Remove recovery requests and
  outcomes. The hook survives REPL rebuilds and runs before native metadata
  preparation. Root help, completion and builtins do not acquire credentials;
  explicit external help passes `before_dispatch`.

- **Breaking:** Remove builtin-policy builders, policy contexts, registries,
  overrides and the mirrored command-policy DSL. `CommandAccess` is `Runnable`, `Denied(reason)` or `Hidden(reason)`.
  Remove the unused `CredentialState::present()` constructor.

- **Breaking:** Remove capability, profile, product-availability, feature,
  visible-session, auth-strength and credential-freshness gates. Protocol-v1
  DTOs still parse the legacy fields and ignore them; `capability_gated` is
  public. Plugins can still require authentication or present/valid credentials.

- **Breaking:** Doctor session JSON drops `auth_strength`, `capability_count`
  and `enabled_features`. Plugin command listings drop `required_capabilities`,
  `feature_flags` and `visible_session`; run requirements and hints show only
  supported fields. Credential validity and TTL diagnostics remain live.

- **Breaking:** Remove `osp_cli::guide` and its guide view, section and entry
  types. Use `core::Document` and semantic blocks instead. Guide restoration
  and the old document-kind API are removed; use `core::OutputDocument` and
  `core::RenderRecommendation::Document` for semantic output.

- **Breaking:** Unstaged help and intro JSON now emit a `blocks` object. Pipes
  emit flat rows carrying `section` and entry descriptions; sorting and limiting
  retain their ordinary row meanings and never reconstruct a help envelope.

### Config migration

Retired keys are ignored with one warning naming them. Remove them with
`osp config unset KEY`. Other unknown keys still fail validation.

- `plugins.<command>.provider`, `plugins.<command>.state`,
  `extensions.plugins.discovery.path`
- `repl.history.profile_scoped`, `session.cache.max_results`
- `ui.prompt`, `ui.prompt.secrets`, `base.dir`
- `ui.help.table_chrome`, `ui.help.entry_indent`, `ui.help.entry_gap`,
  `ui.help.section_spacing`, `ui.medium_list_max`, `ui.indent`, `repl.prompt_right`
- `auth.visible.builtins`, `auth.visible.plugins`
- `ui.message.verbosity`: use `ui.verbosity` instead

`ui.help.level=inherit` is removed; use `normal` or an explicit help level.

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
