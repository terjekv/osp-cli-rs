# cli

The grammar: what a user may type after `osp`, which flags are one-shot
(`--format`, `-v`) and which are config, and the
handlers for builtin commands once a line has parsed. The REPL reuses this
grammar for each line, without process argv.

It parses and runs builtins, but it does not pick between native and plugin
commands or render; that is `app`. It imports widely (`app`, `config`, `core`,
`ui`, …) because builtin handlers need host state.

## Layout

| Path | Owns |
|---|---|
| `mod.rs` | `Cli`, `InlineCommandCli`, the `Commands` enum and shared flags |
| `invocation.rs` | Scanning one-shot invocation flags out of a command line |
| `pipeline.rs` | Alias expansion and splitting `command | stages` |
| `commands/` | Builtin handlers: `config`, `context`, `plugins`, `theme`, `history`, `doctor`, `intro` |
| `rows/` | Helpers that turn handler rows into `OutputResult` |

## Common changes

**Add a builtin command**

1. Add a variant to `Commands` in `mod.rs` with its clap args.
2. Write the handler in `commands/<name>.rs`, returning structured output.
3. Add the dispatch arm in `app/builtin.rs`. Builtins remain public; external
   access belongs to `app::AccessPolicy`.
4. Check REPL completion picks it up: `osp repl debug-complete --line '<name> '`.

Product-specific commands do not go here. A wrapper crate registers them as
native commands.

**Add a one-shot flag:** model it in `invocation.rs` so the CLI, REPL and
tests all see the same flag; REPL completion lists invocation flags from
`repl/completion.rs`. Handlers must not parse their own hidden flags.

Builtin help targets are selected with a passive clap parser, then built from
clap command metadata. Shared invocation help composes document blocks.
`guide` and `--guide` render human documents with the shared document renderer.
