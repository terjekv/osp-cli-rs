# Embedding And Product Wrappers

This guide is for teams building a site-specific product crate on top of
`osp-cli`.

Use this when your product wants:

- the upstream CLI/REPL host
- the upstream config system and output/rendering behavior
- site-specific commands, policy, auth, or integrations

Do not use this guide if you are only writing external plugins. That path is
covered by [PLUGINS.md](PLUGINS.md).

If you want the shortest copy-and-adapt path, read `Recommended Shape` first
and then jump straight to `Worked Recipe`.

The matching runnable example lives in
[`examples/product-wrapper`](../examples/product-wrapper).

## Fast Path

If you want the shortest successful wrapper:

1. Copy [`examples/product-wrapper/src/lib.rs`](../examples/product-wrapper/src/lib.rs)
   and [`examples/product-wrapper/src/main.rs`](../examples/product-wrapper/src/main.rs).
2. Rename `site_*` and `SiteApp` to your product name.
3. Move defaults under `extensions.<product>.*`.
4. Replace `SiteStatusCommand` with one real native command.
5. Keep `main.rs` thin and let `SiteApp::new()` own host wiring.

You can ignore for now:

- manual runtime/session assembly
- the optional `site_runtime_config_for(...)` helper if you do not need
  wrapper-owned tooling/tests outside the host

## API migration

Update wrappers alongside the library checkout. Configure the host through
`App`, and use the narrow `AppRuntime` getters in access-policy hooks.
Host state, editor engines and plugin discovery remain owned by the host.
The unpublished UiO companion uses the sibling path dependency.

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

Native command contexts retain `may_prompt()` and `can_present()`. Terminal
confirmation helpers remain at `ui::prompt::{confirm, confirm_phrase}`.
`AppRuntime::input_is_replay()` identifies sourced commands and redirected stdin; `NativeCommandContext::may_prompt()` rejects both.
`ui::prompt::read_path(label)` opens a history-free REPL file editor; its `completion::path_candidates` lookup also serves `source` and `--from-file`, with tilde paths and explicit dotfile prefixes.
Access hooks can call `AppRuntime::store_secret(key, value)` to use `config set --secrets` storage and reload the host before the pending command runs.
`core::CommandError` carries recovery advice, `ResponseV1::ok` constructs
successful wire responses, and `config::is_sensitive_key` classifies secret
names. Public context contracts and native REPL hooks remain available.

Native commands can implement `configure(&ResolvedConfig)` to select their
local session before help, completion, policy, or execution reads it. Remote
refresh still belongs to execution or completion refresh. Derived UI and auth
state remain owned by the host. Cloned registries share their command instances,
so invocations using `configure` to select mutable session state must be
serialized from configuration through completion. Parallel embedded runs need
independent command instances in separate registries, or service selection
derived from each execution context.

## Ownership Split

Keep reusable CLI behavior in `osp-cli` and site-specific behavior in your
product crate.

Generic mechanism includes:

- CLI and REPL host behavior
- config loading, precedence, and explanation
- rendering and output formatting
- completion/help infrastructure
- native-command registration mechanics

Site-specific facts include:

- auth and policy inputs
- domain integrations
- native commands that expose those integrations
- site-only config under your own namespace

If a change could make sense for another site without changing meaning, it
probably belongs upstream.

## Recommended Shape

To build a wrapper:

1. Keep a thin product-level app type that contains `osp_cli::App`.
2. Expose one wrapper-owned `builder()` that applies your defaults and native
   commands.
3. Build a `NativeCommandRegistry` for your site-specific commands.
4. Build one `ConfigLayer` containing your product-owned defaults under
   `extensions.<site>.*`.
5. Inject both through `App::new()`.
6. Expose a small product API such as `run_process`, `builder`, or
   `assembly`.

Minimal sketch:

```rust
use std::ffi::OsString;

fn site_defaults() -> osp_cli::config::ConfigLayer {
    let mut defaults = osp_cli::config::ConfigLayer::default();
    defaults.set("extensions.site.enabled", true);
    defaults.insert("extensions.site.banner", "cli-wrapper", osp_cli::config::Scope::terminal("cli"));
    defaults
}

#[derive(Clone)]
pub struct SiteApp {
    inner: osp_cli::App,
}

impl SiteApp {
    pub fn builder() -> osp_cli::App {
        osp_cli::App::new()
            .with_native_commands(site_native_registry())
            .with_product_defaults(site_defaults())
    }

    pub fn new() -> Self {
        Self {
            inner: Self::builder(),
        }
    }

    pub fn run_process<I, T>(&self, args: I) -> i32
    where
        I: IntoIterator<Item = T>,
        T: Into<OsString> + Clone,
    {
        self.inner.run_process(args)
    }
}

fn site_native_registry() -> osp_cli::NativeCommandRegistry {
    osp_cli::NativeCommandRegistry::new()
    // .with_command(MyNativeCommand)
}
```

The product crate adds commands and state through its builder.

## Wrapper Checklist

A wrapper needs:

- one wrapper app type owns `osp_cli::App`
- one wrapper `builder()` applies product defaults and native commands
- one namespace owns product-only config: `extensions.<product>.*`
- one registration point owns native commands
- `src/main.rs` delegates into the wrapper crate instead of rebuilding host
  setup inline
- any extra config helper is optional and used only by wrapper-owned tooling,
  tests, or validation

## Worked Recipe

This is the end-to-end pattern to copy and adapt.

It shows:

1. product-owned defaults injected into the same host bootstrap path used by
   native commands and `config` commands
2. the optional matching helper for product-owned tooling that wants to
   resolve config outside the host
3. one complete native command
4. a thin product app wrapper that keeps upstream host startup behavior

Keep the file split simple:

- `src/lib.rs` owns defaults, native command registration, and the wrapper app
- `src/main.rs` should usually be a one-line delegate into that wrapper
- the config helper is for tooling/tests, not for ordinary startup

```rust
use std::ffi::OsString;

use anyhow::Result;
use clap::Command;
use osp_cli::config::{
    ConfigError, ConfigLayer, ResolveOptions, ResolvedConfig, RuntimeConfigPaths,
    RuntimeDefaults, RuntimeLoadOptions, build_runtime_pipeline,
};
use osp_cli::{
    App, NativeCommand, NativeCommandContext, NativeCommandOutcome,
    NativeCommandRegistry,
};

fn site_defaults() -> ConfigLayer {
    let mut layer = ConfigLayer::default();
    layer.set("extensions.site.enabled", true);
    layer.insert("extensions.site.banner", "cli-wrapper", osp_cli::config::Scope::terminal("cli"));
    layer
}

fn site_runtime_config_for(terminal: &str) -> Result<ResolvedConfig, ConfigError> {
    let paths = RuntimeConfigPaths::discover();
    let mut defaults = RuntimeDefaults::from_process_env("dracula", "site> ").to_layer();
    defaults.extend_from_layer(&site_defaults());

    build_runtime_pipeline(
        defaults,
        None,
        &paths,
        RuntimeLoadOptions::default(),
        None,
        None,
    )
    .resolve(ResolveOptions::new().with_terminal(terminal))
}

struct SiteStatusCommand;

impl NativeCommand for SiteStatusCommand {
    fn command(&self) -> Command {
        Command::new("site-status").about("Show site-specific status")
    }

    fn execute(
        &self,
        _args: &[String],
        context: &NativeCommandContext<'_>,
    ) -> Result<NativeCommandOutcome> {
        Ok(NativeCommandOutcome::Help(format!(
            "site enabled: {}",
            context
                .config
                .get_bool("extensions.site.enabled")
                .unwrap_or(false)
        )))
    }
}

fn site_native_registry() -> NativeCommandRegistry {
    NativeCommandRegistry::new().with_command(SiteStatusCommand)
}

#[derive(Clone)]
pub struct SiteApp {
    inner: App,
}

impl SiteApp {
    pub fn builder() -> App {
        App::new()
            .with_native_commands(site_native_registry())
            .with_product_defaults(site_defaults())
    }

    pub fn new() -> Self {
        Self {
            inner: Self::builder(),
        }
    }

    pub fn run_process<I, T>(&self, args: I) -> i32
    where
        I: IntoIterator<Item = T>,
        T: Into<OsString> + Clone,
    {
        self.inner.run_process(args)
    }
}
```

Delegate from `src/main.rs` to the wrapper:

```rust
fn main() {
    std::process::exit(site_product::SiteApp::new().run_process(std::env::args_os()));
}
```

Notes:

- the wrapper crate owns `extensions.site.*` defaults and native commands
- `osp_cli::App` still owns the generic host behavior
- `with_product_defaults(...)` pushes wrapper defaults into the same runtime
  bootstrap path used by native commands, help rendering, `config get`, and
  `config explain`
- `site_runtime_config_for(terminal)` is optional and meant for product-owned
  tests, validation, and adjacent tooling that need the same merged defaults
  outside the host
- `SiteApp::new()` gives callers one place to construct the configured host
- if the product does not need its own preflight config step, keep the wrapper
  thinner and only inject the native registry and product defaults

## Common Mistakes

Avoid these wrapper-crate mistakes:

- putting product-only keys at the top level instead of under
  `extensions.<product>.*`
- resolving wrapper config in a side channel while the host reads a different
  config snapshot
- putting startup wiring directly in `main.rs`
- leaking site-specific state into `osp-cli::app` instead of keeping it in the
  wrapper crate
- forking generic help/completion/config behavior when native commands and
  product defaults are enough

## Config Strategy

Do not create a second config system in the wrapper crate.

The intended approach is:

- keep product-only keys under a dedicated namespace such as
  `extensions.<site>.*`
- continue using upstream `ConfigLayer`, `LoaderPipeline`,
  `ConfigResolver`, and `ResolvedConfig`
- use `RuntimeDefaults`, `RuntimeConfigPaths`, and
  `build_runtime_pipeline` when you need stock host-style bootstrap

If you need product-specific defaults, inject them as normal config layers.
Do not fork source precedence or file-discovery rules unless the product
really needs a different contract.

## Native Commands

Use native commands when the product wants built-in commands that participate
in the host's help, completion, policy checks, and dispatch.

When implementing native commands:

- command implementations should consume a `NativeCommandContext`
- write follow-up commands with `context.command("orch task wait 42")`, using
  root-form input so terminal and REPL readers receive the appropriate spelling
- long-running commands should emit `NativeProgressEvent` values through the
  context rather than writing directly to stdio
- streaming endpoints call `context.emit_stream(event)` with one typed JSON
  object in `event.data`; human output uses the shared renderer and `--json`
  writes each event as a compact JSON line on stdout before the final result
  document. DSL stages apply to each event and to the final result separately;
  filtered-out events emit nothing. Events are never coalesced, including replay
  events, and quiet mode does not suppress machine-readable events
- use `event.with_meta(...)` for event columns/labels/format and optionally
  `event.with_frame(snapshot, meta)` for an accumulated structured terminal
  frame. The shared renderer renders that snapshot only on an unfiltered human
  terminal; JSON, redirected output and DSL always consume the original event.
  Set `progress_replace: true` on the frame metadata to replace it safely
- `emit_progress` remains preliminary human progress on stderr, independent of
  the streaming endpoint contract. Return the final document through the normal
  `NativeCommandOutcome` path; stream metadata never replaces that document
- use structured data and `progress_replace: true` for preliminary status;
  interactive stderr clears it when the command finishes or presents another
  document
- call `context.flush_progress()` before blocking for input, and use
  `context.present(...)` for notices and confirmations that must remain visible
  outside result-stream DSL filters; contexts without a progress sink make
  these calls no-ops
- use `NativeCommandOutcome::ResponseWithExit` when a stable final document
  must accompany a meaningful non-zero outcome
- product-specific state should live in the wrapper crate, not in
  `osp-cli::app`
- use `NativeCommand::configure` to select local service/session state from the
  resolved host config before help, policy, completion, and execution inspect
  it; keep remote refresh out of this hook
- Keep credentials in the product's shared credential owner. Return current
  authentication and credential facts from `AccessPolicy::session_facts`; use
  `AppRuntime::set_authenticated_subject` to publish the validated subject.
  See [AUTH.md](AUTH.md#product-hook) for the acquisition lifecycle.
- registration should happen in one place, usually a product integrations
  module

## What Not To Fork

Avoid downstream copies of:

- config precedence logic
- CLI parsing and REPL shell behavior
- help/completion/catalog wiring
- plugin protocol or subprocess dispatch mechanics
- renderer decisions and output shape rules

If those need to change generically, upstream is the right place.

## Pointers

For rendered API docs, prefer docs.rs or `cargo doc --open`.

For a runnable rustdoc version of the minimal wrapper pattern, see the crate
root in [`src/lib.rs`](../src/lib.rs).

For a copyable wrapper crate, start with
[`examples/product-wrapper/src/lib.rs`](../examples/product-wrapper/src/lib.rs)
and [`examples/product-wrapper/src/main.rs`](../examples/product-wrapper/src/main.rs).

For code-level entrypoints, start with:

- [`src/lib.rs`](../src/lib.rs)
- [`src/app/mod.rs`](../src/app/mod.rs)
- [`src/native.rs`](../src/native.rs)
- [`src/config/mod.rs`](../src/config/mod.rs)
- [`src/config/runtime.rs`](../src/config/runtime.rs)

## Config loader composition

`osp_cli::config` exposes a generic loader interface so each source can be wired
without custom host loading code:

- `config::ConfigLoader` trait (`load() -> Result<ConfigLayer, ConfigError>`)
- `StaticLayerLoader` for in-memory defaults/session layers
- `TomlFileLoader` for file-backed layers (optional or required)
- `EnvVarLoader` for `OSP__...` environment overrides
- `SecretsTomlLoader` for file-backed secrets with strict permissions checks
- `EnvSecretsLoader` for `OSP_SECRET__...` secret overrides
- `ChainedLoader` for combining multiple backends within one precedence stage
- `LoaderPipeline` for assembling all loaders in precedence order

## REPL product hooks

### Background notices

Products can use `osp_cli::repl::post_notice` for a short informational line
computed after startup. The editor prints it above the prompt; work posted
before the editor starts is queued. The basic reader also displays notices on
stderr, although it cannot redraw a partially typed line. Notice delivery does
not wait for network work or block the prompt.

### Product status

Native commands may supply `repl_status()` as cached status text in a separate
right-prompt slot. Rendering never performs network I/O. The host sanitizes
control sequences and line breaks; Reedline measures terminal cells and omits
the right prompt when it collides with input. Context and incognito remain;
product attention takes priority over timing. Basic and JSON sessions omit it.
Product `repl_status` hooks return `ReplStatus` text and a message level.
The host strips terminal controls before applying the active theme's error
or warning colour. `NO_COLOR`, `--no-color` and plain rendering suppress colour.
A cached update is visible at the next editor paint. The pinned Reedline reader
blocks on terminal input: cache changes and queued
transition notices appear at the next input event, rather than waking an idle
reader. No fake input events or worker ANSI writes are used.

### Product actions and terminal ownership

Internally, the editor distinguishes ordinary lines, global history selections
that bypass shell scope, and named actions. Products use native-command hooks
to register `repl::ReplAction`s and implement
`NativeCommand::execute_repl_action`; the host uses ordinary elevation without
running an interactive login hook. Actions bypass user history, alias expansion,
shell navigation, last-command expansion and pagination reset. The same editor
retains its draft and byte cursor across normal return, cancellation and errors.

Bindings accept F2–F12, Alt+Enter, Alt+L, Alt+E, Alt+Y or `none`; duplicate action bindings are rejected. Alt letter bindings override the editor defaults.
Products supply a default and a human label. Basic and JSON
sessions have no product shortcuts. Existing Ctrl+R, Ctrl+L and pagination keys
keep their meanings.

`NativeCommand::start_repl(config, Some(ReplSession))` supplies a cancellable
rich-editor lifetime.
Basic selection and fallback report `None` for legacy startup behavior only;
recurring attention and sound remain disabled.
Workers use its bounded, sanitized `post_notice(text)` sink and
`wait(duration)` cancellation point. Reedline queues delivery while foreground
commands, line prompts, raw menus, pagers or the history picker own the terminal.
Rebuild, fallback and exit cancel the handle and discard queued product output.
Workers must also validate their own credential generation before publication.
