# Logging, Verbosity, and Debugging

`osp` separates user-facing message detail from developer-facing debug output.

## User-Facing Verbosity

Use:

- `-v`
- `-vv`
- `-vvv`
- `-q`
- `-qq`

Behavior:

- default: errors, warnings, and normal success messages
- `-v`: more detail plus cause/context chain on failures
- `-vv`: rich diagnostic failure output, with snippets when available
- `-vvv`: rich diagnostic failure output plus stack backtrace
- `-q`: fewer non-essential messages
- `-qq`: quiet except for errors

These flags work in both CLI and REPL and apply to one invocation at a time.

Important split:

- `-v/-vv/-vvv` change how failures are rendered
- `-d/-dd/-ddd` change developer logs and timing

They are related but not interchangeable. A command can have a terse failure
and high debug logging, or a forensic failure render without extra debug logs.

## Developer Debug Output

Use:

- `-d`
- `-dd`
- `-ddd`

Behavior:

- `-d`: high-level debug information
- `-dd`: more detailed debug output
- `-ddd`: the most detailed terminal debug stream

Debug logs are written to `stderr`, not `stdout`.

## Timing Hints

At debug levels, `osp` shows timing information:

- CLI: timing footer on `stderr`
- REPL: right-hand prompt badge with the last command timing

Color bands:

- `0-250ms`: green
- `250-1000ms`: warning
- `>1000ms`: red

Higher debug levels may show more detailed timing breakdowns.

## Startup Tracing

Set `OSP_TRACE_STARTUP=1` to write a human-readable phase tree to `stderr`, or
set it to `json` for one JSON document. The setting is read once when the
process starts; it is off by default. The timing report is separate from `-d`
developer logs and does not change their format. Startup phases are tracing spans
and also provide context in developer logs.

Each phase reports its start offset, elapsed duration, nesting depth, parent,
and share of the total. Phase durations are inclusive, so nested phase shares
can add up to more than 100%. A REPL trace ends at the final callback that
assembles its first prompt, before the editor waits for input. A one-shot trace
ends as the process wrapper returns and includes command execution.

```bash
OSP_TRACE_STARTUP=1 osp
OSP_TRACE_STARTUP=json osp --defaults-only config show 2>startup.json
```

Use this trace to find repeated config passes, plugin discovery and `--describe`
subprocesses, or work that runs before the command is known.

## REPL Behavior

In the REPL, `-v/-q/-d` are command-local. They do not permanently change the
shell’s behavior unless you change config defaults. Launch flags set the
session baseline; per-line flags add to that baseline for one command. The
stderr override leaves file logging unchanged.

```text
plugins commands -v
plugins commands -vv
plugins commands -dd
```

## Config Keys

Useful config keys:

- `ui.verbosity.level`
- `debug.level`
- `log.file.enabled`
- `log.file.path`
- `log.file.level`

Use `config explain <key>` to see where an effective value comes from.

If you need the full colored forensic error render, prefer invocation flags
such as `-vvv`. That is intentionally a per-command decision rather than a
sticky logging default.

## Plugin Runtime Hints

`osp` passes logging-related hints to plugins:

- `OSP_UI_VERBOSITY`
- `OSP_DEBUG_LEVEL`

Plugins should keep data output on `stdout` and diagnostics on `stderr`.

## Scripting Safety

`--format json` stays safe for scripts even when `-d` is enabled because data
stays on `stdout` and diagnostics stay on `stderr`.

```bash
osp plugins commands --json -d 2>debug.log
```
