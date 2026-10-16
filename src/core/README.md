# core

Small shared types that must mean the same thing everywhere: a row, a command
result, a command description, the plugin wire DTOs, shell quoting and fuzzy
matching. Everything else builds on these.

Imports nothing else from the crate, and must stay that way. A type belongs
here only if several modules need it and its meaning will not change with any
one feature. `core` is not a place for helpers without a home.

## Layout

| File | Owns |
|---|---|
| `row.rs` | `Row`, the record type commands, the DSL and the UI pass around |
| `document.rs` | Ordered semantic blocks shared by help, intro, and UI rendering |
| `output_model.rs` | `OutputResult`, JSON/row conversion, group context and render hints |
| `output.rs` | Output format and mode enums shared by flags, config and UI |
| `command_policy.rs` | Whether a command is visible or allowed in the current context |
| `plugin.rs` | Plugin protocol DTOs (`DescribeV1`, `ResponseV1`) and validation |
| `runtime.rs` | Runtime hints passed to native and plugin commands |
| `shell_words.rs`, `fuzzy.rs` | Quoting for display; the fuzzy matcher used by completion |

## Common changes

- **Protocol change:** `plugin.rs` together with `docs/PLUGIN_PROTOCOL.md`.
  Additive fields with defaults keep old plugins working; anything else needs
  a protocol version bump.
- **New command metadata (e.g. a flag property):** declare it in clap for
  native commands and builtins. Update the direct projections in
  `completion/tree.rs` and `document.rs`; protocol metadata belongs in
  `plugin.rs` at the plugin boundary.

`Document` is the semantic help/intro model. Its clap metadata adapter, external
text parser, and Markdown template parser produce ordered blocks. Its row
projection carries section context explicitly, with no guide buckets or restore.
