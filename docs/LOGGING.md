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
- `-v`: full failure report, including causes, backend details and source snippets
- `-vv`: same detailed failure report as `-v`, with trace-level messages
- `-vvv`: same as `-vv`; use `-dd` for captured backtraces
- `-q`: fewer non-essential messages
- `-qq`: quiet except for errors

These flags work in both CLI and REPL and apply to one invocation at a time.

`-v`/`-q` set how much osp tells you: messages, failure explanations, human
field detail and help. `-d` shows what osp is doing. The axes compose freely;
`-q -d` keeps quiet messages with operator logs. Further counts saturate.

## Developer Debug Output

Use:

- `-d`
- `-dd`
- `-ddd`

Behavior:

- `-d`: command dispatch, service requests, plugin stderr and the final outcome
- `-dd`: internal debug events and the error’s captured backtrace
- `-ddd`: the most detailed terminal debug stream

Debug logs are written to `stderr`, not `stdout`. Operator trace lines contain
the time, level, target, message and event fields. At `-dd` and above they also
include span context.

## Timing Hints

At debug levels, `osp` shows timing information:

- CLI: `finished` log event on `stderr`, after any error, with exit code and elapsed time
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
- `log.file.level` (default `info`, independent of `-d`)

Use `config explain <key>` to see where an effective value comes from.

For a bug report, use `-v -dd 2>osp.txt` to capture the full explanation,
operator trace, internal events and a captured backtrace. `doctor last -v`
shows the saved REPL report; `doctor last -dd` writes its backtrace to stderr.

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
