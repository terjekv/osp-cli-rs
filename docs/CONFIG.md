# Config and Profile Resolution

`osp` resolves config from multiple sources with explicit precedence, profile
scoping, terminal scoping, and separate secret storage.

This document is for answering operational questions such as:

- why did this value win?
- where can I set this key?
- what changes only this invocation versus stored config?
- which values are runtime data versus bootstrap inputs?

## Overview

Key properties:

- multiple sources with explicit precedence
- profile and terminal scoping
- placeholder interpolation for strings
- separate secret storage
- diagnostics that show where values came from
- strict schema validation with `extensions.*` as the open namespace

## Which Command Should I Run?

Use the smallest command that answers your question:

- `config show`
  - "What does the resolved runtime config look like right now?"
- `config get <key>`
  - "What is the winning value for this one key?"
- `config explain <key>`
  - "Why did this value win over the other candidates?"
- `config set <key> <value>`
  - "Change the current session or a persistent store."
- `config unset <key>`
  - "Remove an explicit value so the next source can win."

Useful switches:

- `--sources`
  - include winner provenance
- `--raw`
  - show pre-interpolation values
- `--session`
  - force an in-memory change
- `--permanent`
  - persist the change when running inside the REPL

## Interactive editing

Run `osp config edit` in a terminal, or `osp config edit ui.format` to start on
a particular key. The right pane shows the staged value, current source layer
and schema documentation. Up/Down select keys; Tab or Left/Right cycle boolean
and enum values. Enter edits other values inline; Ctrl-U clears the editor.

End focuses `[ Save ]`. Enter applies changed keys through `config set`, using
its default store and scope (including `config.default-target`). In a REPL this
normally changes the session only. `[ Exit ]`, Esc and q discard staged edits.
Secret values remain redacted and inline secret input is masked. Validation
errors stay in the picker. The picker needs a terminal of at least 60×12;
`config get KEY` and `config set KEY VALUE` work non-interactively.

## Display names

Set the current user's greeting name; quotes are optional:

```bash
osp config set user.display_name Øistein Søvik
osp config set user.display_name Øistein Søvik --user oistes-drift
```

The setter's `-u` / `--user` option selects another account without changing
the session user. Otherwise the current user (including root `osp -u USER`)
is used. All value words are joined with spaces; there is no positional
username after the name. Values are stored separately under
`user.display_names.<username>`; `{{display_name}}` uses the selected user's
entry and falls back to the username. Usernames containing dots are unsupported
in this table. Old stored `user.display_name` and `user.full_name` keys must be
replaced with the per-user key.

## Profile Selection On The Command Line

Select a configured profile explicitly:

```bash
osp --profile tsd plugins list
```

Without `--profile`, configuration selects `profile.default`. A profile name
is never a positional prefix: `osp tsd` invokes a command named `tsd`.
Profiles cannot shadow commands.

Known profiles come from configured profile scopes and defaults. Unknown
explicit profiles and unknown environment-selected defaults fail with
`UnknownProfile` during configuration resolution, before command/network work.
They never silently fall back to another environment.

## Scoping Rules (Profile + Terminal)

Within a single loader, the resolver picks the most specific match using this
order:

1. (key, profile, terminal)
2. (key, profile, none)
3. (key, none, terminal)
4. (key, none, none)

This rule is applied uniformly to all loaders.

## Loader Precedence

Lowest to highest priority, later sources override earlier ones:

1. Built-in defaults
2. Config file
3. Secrets store
4. Environment variables
5. Session overrides (in-memory)

An `OSP_SECRET__...` environment override is an exception: it wins over a plain
`OSP__...` environment value for the same key. Session overrides still win.

This order, including that exception, is the public contract. Changing it is a
breaking change.

Notes:

- REPL `config set --session ...` writes into the in-memory session layer.
- Launch-time bootstrap flags like `--theme` and `-u` may still affect startup
  state or session defaults.
- Invocation flags like `--json`, `--format`, `--render-mode`, `--color`, `--ascii`,
  `-v/-q` and `-d` do not write into config state.
- `config get --sources` therefore reflects stored defaults, not one-shot
  invocation overrides.

For UI keys, there is one more rule inside the resolved config:

- explicit per-key values beat `ui.presentation`
- `ui.presentation` only seeds keys still at builtin default
- `config explain` shows that seeded effect when it matters

See [EMBEDDING.md](EMBEDDING.md#config-loader-composition) for loader composition.

## Typed Schema and Validation

The host validates resolved keys against its internal typed schema:

- Unknown keys are rejected by default.
- `extensions.*` is the only open namespace for unknown keys.
- Plugin environment config uses `extensions.plugins.env.*` and
  `extensions.plugins.<command>.env.*`. See [PLUGINS.md](PLUGINS.md).
- `extensions.plugins.timeout_ms` bounds optional `--osp-describe` calls only.
  Default: `10000`. Ordinary runs have no timeout.
- `extensions.plugins.disabled` lists disabled command names. PATH discovery
  is enabled by default; there is no discovery toggle in config.
- The `extensions.plugins.*` namespace accepts the keys above and environment
  projection keys. Other extension namespaces remain open.
- Values are adapted to schema type after merge/interpolation:
  strings, booleans, integers, floats.
- Enum-like string keys (for example `ui.format`) are validated against
  allowed values.
- Required runtime keys are enforced (`profile.active`).
- Bootstrap-only keys are validated during bootstrap, not treated as ordinary
  runtime-resolved values.

## Resolution Pipeline

1. Run path bootstrap from pre-file inputs:
   CLI/env/session/bootstrap context/platform defaults.
2. Load all configured layers.
3. Run profile bootstrap:
   CLI `--profile` override or `profile.default` from bootstrap-safe scopes.
4. Merge runtime values using the precedence list above.
5. Interpolate placeholders in string values.
6. Adapt types using the schema.
7. Compute derived values in code, not in config.
8. Validate required runtime keys and freeze the result.

Bootstrap is expressive by layer, but restricted by scope:

- path bootstrap does not read config-file contents
- profile bootstrap reads across loaded layers
- profile bootstrap ignores profile-dependent scopes
- runtime resolution never changes the active profile

## Placeholders

Placeholders are string-only `${key}` references and are resolved after all
sources are merged. Rules:

- Unresolved placeholders are errors.
- Cycles are errors.
- Placeholders can reference other placeholders.
- Placeholders may reference secrets but must be redacted in diagnostics.
- `config set` validates the proposed value before changing a session or store,
  using this invocation's selected values, including dry runs and values currently
  masked by higher-priority sources.

If a stored placeholder is broken, `config show --raw` and `config unset` load
selected values without interpolation or schema adaptation so the file can be
inspected and repaired. `doctor` uses the same raw view and reports resolution
errors in its config diagnostics. Normal commands still require valid resolution.

## TOML Structure

This is the canonical file layout:

```toml
[default]
profile.default = "uio"
ui.format = "table"

[profile.uio]
extensions.plugins.env.api.url = "https://api.example.org"

[profile.tsd]
ui.format = "json"

[terminal.repl]
profile.default = "tsd"
repl.history.enabled = true

[terminal.repl.profile.tsd]
ui.format = "table"
```

Notes:

- The profile list is derived from the `[profile.*]` tables. Removing the last
  value from a profile keeps its declaration so it remains selectable.
- Unscoped keys live in `[default]`.
- Bootstrap keys such as `profile.default` may be set in `[default]` and
  `terminal.<term>`.
- Bootstrap keys such as `profile.default` must not be set in `[profile.*]` or
  `terminal.<term>.profile.<name>`.
- Fully scoped values use `terminal.<term>.profile.<name>`.

Config edits preserve comments and formatting outside the changed values, follow
existing symlinks to their targets, and retain existing config-file permissions.

## Environment Variable Mapping

Environment names map to configuration keys as follows:

- `OSP__UI__FORMAT` -> `ui.format`
- `OSP__PROFILE__TSD__UI__FORMAT` -> `ui.format` scoped to profile `tsd`
- `OSP__TERM__REPL__UI__FORMAT` -> `ui.format` scoped to terminal `repl`
- `OSP__TERM__REPL__PROFILE__TSD__UI__FORMAT` -> fully scoped

All env values are strings and are adapted by the schema.
Profile and terminal identifiers are case-insensitive and normalized to
lowercase at parse time.

Secret env mapping uses the same scope grammar with `OSP_SECRET__`:

- `OSP_SECRET__SERVICE__API_TOKEN` -> `service.api_token`
- `OSP_SECRET__PROFILE__TSD__SERVICE__API_TOKEN` -> profile-scoped secret
- `OSP_SECRET__TERM__REPL__PROFILE__TSD__SERVICE__API_TOKEN` ->
  terminal+profile scoped secret

## Secrets

Secrets are stored in a separate backend:

- Default backend: TOML file with `0600` permissions (`SecretsTomlLoader`).
- Set `secrets.backend = "keyring"` in ordinary config to use the native
  platform credential store instead. Backend selection is resolved only from
  non-secret bootstrap sources, so the secrets store cannot select itself.
- The keyring backend keeps values in Keychain, Credential Manager, or Secret
  Service. `${XDG_CONFIG_HOME}/osp/secrets.index.toml` contains only the
  normalized key/profile/terminal tuples needed to enumerate those values; it
  is written atomically with `0600` permissions.
- Optional override backend: `OSP_SECRET__...` environment variables
  (`EnvSecretsLoader`).
- Sensitive keys belong in the secrets store, but `config set` only writes
  there when you choose `--secrets`.
- Without `--secrets`, `config set` warns and still writes the value to the
  main config store.
- Secrets are wrapped in a redacted type for diagnostics.

Import a value without putting it in shell history or process arguments:

```bash
osp config set extensions.example.token --secrets --from-file ~/token
pass show example | osp config set extensions.example.token --secrets --from-file -
```

`--from-file PATH` reads a UTF-8 file once; `-` reads stdin until EOF. It is
mutually exclusive with a positional value. Only trailing CR/LF characters
are removed; other whitespace is preserved. The normal schema validation,
scoping and store selection still apply. With `--secrets`, the selected file
or keyring backend owns the value. Without it, the sensitive-key warning
still applies. The source file is never consulted again; repeat the import
to rotate the value.

No secret defaults are allowed in code. Missing secrets must surface as
clear config errors.

`OSP_SECRETS_FILE` overrides the TOML store path and
`OSP_SECRETS_INDEX_FILE` overrides the keyring index path. Switching backends
does not read or merge the inactive backend. Migrate values explicitly before
changing `secrets.backend`; product integrations must only consume resolved
config values and must not probe legacy token paths themselves.

## Derived Values

Derived values are computed after resolution and are not configurable:

- `app.config_dir`, `app.data_dir`, `app.log_dir`
- `profile.active`
- resolved prompt strings

Derived values must never depend on values that can change at runtime without
rebuilding the config.

`profile.default` is not a runtime-derived value. It is a bootstrap input and
is not carried as an ordinary resolved runtime key.

## Validation and Diagnostics

The resolver should provide:

- `config show` for resolved runtime values
- `config show --sources` to include source + scope
- `config show --raw` for pre-interpolation values
- `config get <key>` and `config get <key> --sources`
- `config explain <key>` for winner + precedence chain + interpolation trace
- bootstrap-aware explain for bootstrap keys such as `profile.default`
- explain output should label whether a key was resolved in bootstrap or runtime
- JSON explain output includes `"phase": "bootstrap"` or `"phase": "runtime"`
- bootstrap-only keys stay out of ordinary runtime `config show`
- `config diagnostics` summary

Diagnostics must redact secrets and avoid logging token contents.

## UI/REPL Keys (Current)

These keys currently drive user-visible rendering and REPL presentation:

- `ui.presentation`
  - `expressive | compact | austere`
- `ui.format`
- `ui.guide.default_format`
  - `guide | inherit | none`; defaults to `guide`. Uses the document renderer
    for help-like output; `inherit` and `none` follow the configured format.
- `ui.mode`
- `ui.color.mode`
- `ui.unicode.mode`
- `ui.width`
  - Optional explicit render width; with a positive `ui.width-max`, the
    actual narrower terminal also limits output.
- `ui.width-max`
  - Maximum rendered width; defaults to `-1` (natural content width).
    A positive limit fits output to that limit or the narrower terminal.
    `0` also disables the maximum. An explicit `ui.width` still applies.
- `ui.chrome.frame`
  - `none | top | bottom | top-bottom | square | round`
- `ui.chrome.rule_policy`
  - `per-section | shared`
- `ui.table.border`
  - `none | square | round`
- `ui.table.nested_border`
  - `inherit | none | square | round`; defaults to `none`. Applies to tables
    nested inside a record (`addresses (2):`); `inherit` follows `ui.table.border`
- `ui.table.overflow`
  - `clip | ellipsis | wrap | none`; defaults to `ellipsis`
- `ui.help.level`
  - `none | tiny | normal | verbose` (default `normal`, adjusted by `-v`/`-q`)
- `theme.name`
- `theme.path`
- `user.name`
- `domain`
- `repl.prompt`
- `repl.exit_message`
  - printed when leaving the root REPL; empty by default
- `repl.simple_prompt`
- `repl.shell_indicator`
- `repl.intro`
  - `none | minimal | compact | full`
- `ui.messages.layout`
  - `full | compact | austere | plain | none`
  - `grouped` and `minimal` are aliases for `full` and `austere`

## UI Examples

Compact REPL defaults:

```toml
[terminal.repl]
ui.presentation = "compact"
```

Quiet, plain operator profile:

```toml
[profile.ops]
ui.presentation = "austere"
ui.mode = "plain"
ui.color.mode = "never"
ui.chrome.frame = "none"
ui.messages.layout = "minimal"
```

Compatibility note:

- if you used `gammel-og-bitter` before, use `ui.presentation = "austere"`
- the old name remains a CLI alias, not the canonical config vocabulary

## REPL Config Writes

Without a configured `config.default-target` or explicit scope/store flags,
store choice depends on where you run the command:

- in one-shot CLI, `config set` defaults to the persistent config store
- in the REPL, `config set` defaults to the session store
- use `--permanent`, `--config`, or `--secrets` for persistence
- use `--session` to force in-memory session behavior

`config.default-target` chooses the default destination for future writes:
`session` is temporary, `global` is saved for all profiles, and any other value
names a saved profile. Explicit `--session`, `--profile`, `--global`, or
`--permanent` flags override the corresponding default. `--terminal repl` or
`--terminal cli` further narrows applicability. Without a configured default,
REPL writes are temporary and one-shot writes persist for the active profile.

```text
config set config.default-target uio --permanent --global
config set ui.margin 2
config set ui.margin 4 --session
```

The first command saves the default destination for all profiles. Subsequent
plain writes go to profile `uio`; the last command remains temporary. Successful
temporary writes show a concrete `--permanent` command with the resolved scope.
`config set --dry-run --explain` retains the proposed value and destination and
adds a `current` explanation of the key before the write.

## Fixed presentation settings in v2

See [UI.md](UI.md#fixed-layout-defaults) for fixed presentation settings.

Retired keys are ignored with one warning naming them. Remove them with
`osp config unset KEY`. Other unknown keys still fail validation. See
[CHANGELOG.md](../CHANGELOG.md) for the removal list.
